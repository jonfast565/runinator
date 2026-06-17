use crate::{
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ResultMetadata,
        validate_provider_metadata,
    },
    types::{RuninatorField, RuninatorType},
    value::Value,
    workflow_state::{DebugFrame, DebugMode, WorkflowRunState},
    workflows::*,
};
use serde_json::json;

// the split DebugFrame must round-trip through a single flat `debug` object so persisted state
// and the frontend keep seeing the same wire shape.
#[test]
fn debug_frame_flattens_to_flat_object() {
    let blob: Value = json!({
        "debug": {
            "enabled": true,
            "paused": true,
            "step_requested": false,
            "mode": "breakpoints",
            "breakpoints": ["end"],
            "one_shot_breakpoint": "mid",
            "current_node_id": "start",
            "last_output_json": { "ok": true }
        }
    })
    .into();

    let parsed = WorkflowRunState::from_state(&blob);
    let debug = parsed.debug.clone().expect("debug frame present");
    assert!(debug.config.enabled);
    assert_eq!(debug.config.mode, Some(DebugMode::Breakpoints));
    assert_eq!(debug.config.breakpoints, vec!["end".to_string()]);
    assert!(debug.runtime.paused);
    assert!(!debug.runtime.step_requested);
    assert_eq!(debug.runtime.one_shot_breakpoint.as_deref(), Some("mid"));
    assert_eq!(debug.runtime.current_node_id.as_deref(), Some("start"));

    // re-serialize and confirm the keys stay flat under `debug`.
    let back = parsed.to_state();
    assert_eq!(back["debug"]["mode"], Value::from(json!("breakpoints")));
    assert_eq!(back["debug"]["paused"], Value::from(json!(true)));
    assert_eq!(back["debug"]["breakpoints"], Value::from(json!(["end"])));
    assert_eq!(
        back["debug"]["one_shot_breakpoint"],
        Value::from(json!("mid"))
    );
}

#[test]
fn debug_mode_defaults_to_step_all() {
    let frame = DebugFrame::default();
    assert_eq!(frame.config.mode.unwrap_or_default(), DebugMode::StepAll);
}

// the custom value must serialize byte-identically to serde_json::Value so the http edge, the
// database text columns, and the frontend keep seeing the same wire form.
#[test]
fn value_round_trips_byte_identically_to_serde_json() {
    let raw = r#"{"b":2,"a":{"nested":[1,2.5,null,"x"],"flag":true},"z":-7,"big":9007199254740993,"empty":{}}"#;
    let theirs: serde_json::Value = serde_json::from_str(raw).unwrap();
    let ours: Value = serde_json::from_str(raw).unwrap();
    // sorted keys + number formatting must match exactly.
    assert_eq!(
        serde_json::to_string(&ours).unwrap(),
        serde_json::to_string(&theirs).unwrap()
    );
    // and the bridge in either direction is lossless.
    assert_eq!(serde_json::Value::from(ours.clone()), theirs);
    assert_eq!(Value::from(theirs), ours);
}

#[test]
fn workflow_status_terminal_and_active() {
    assert!(WorkflowStatus::Succeeded.is_terminal());
    assert!(!WorkflowStatus::Succeeded.is_active());
    assert!(WorkflowStatus::Failed.is_terminal());
    assert!(WorkflowStatus::TimedOut.is_terminal());
    assert!(WorkflowStatus::Canceled.is_terminal());

    assert!(!WorkflowStatus::Queued.is_terminal());
    assert!(!WorkflowStatus::Paused.is_terminal());
    assert!(WorkflowStatus::Queued.is_active());
    assert!(WorkflowStatus::Running.is_active());
    assert!(WorkflowStatus::DebugPaused.is_active());
    assert!(WorkflowStatus::Waiting.is_active());
    assert!(WorkflowStatus::ApprovalRequired.is_active());

    assert!(!WorkflowStatus::Blocked.is_terminal());
    assert!(!WorkflowStatus::Paused.is_active());
    assert!(!WorkflowStatus::Blocked.is_active());
}

#[test]
fn workflow_status_serialization() {
    let status = WorkflowStatus::ApprovalRequired;
    let serialized = serde_json::to_string(&status).unwrap();
    assert_eq!(serialized, "\"approval_required\"");

    let deserialized: WorkflowStatus = serde_json::from_str("\"approval_required\"").unwrap();
    assert_eq!(deserialized, WorkflowStatus::ApprovalRequired);

    let debug: WorkflowStatus = serde_json::from_str("\"debug_paused\"").unwrap();
    assert_eq!(debug, WorkflowStatus::DebugPaused);
    assert_eq!(debug.as_str(), "debug_paused");

    let paused: WorkflowStatus = serde_json::from_str("\"paused\"").unwrap();
    assert_eq!(paused, WorkflowStatus::Paused);
    assert_eq!(paused.as_str(), "paused");
}

#[test]
fn workflow_node_serialization() {
    let node_json = json!({
        "id": "test-node",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
        "transitions": {
            "on_success": { "$node": "next-node" }
        }
    });
    let node: WorkflowNode = serde_json::from_value(node_json).unwrap();
    assert_eq!(node.id, "test-node");
    assert_eq!(node.kind, WorkflowNodeKind::Action);
    assert_eq!(
        node.action.as_ref().map(|action| action.provider.as_str()),
        Some("console")
    );
    assert_eq!(
        node.transitions
            .on_success
            .as_ref()
            .map(WorkflowNodeRef::as_str),
        Some("next-node")
    );
}

#[test]
fn workflow_node_accepts_reentry_configuration() {
    let node: WorkflowNode = serde_json::from_value(json!({
        "id": "build",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
        "reentry": {
            "enabled": true,
            "max_visits": 3,
            "on_exhausted": { "$node": "deferred" }
        }
    }))
    .unwrap();

    assert!(node.reentry.enabled);
    assert_eq!(node.reentry.max_visits, 3);
    assert_eq!(
        node.reentry
            .on_exhausted
            .as_ref()
            .map(WorkflowNodeRef::as_str),
        Some("deferred")
    );
}

#[test]
fn workflow_action_accepts_inline_configuration_items() {
    let node: WorkflowNode = serde_json::from_value(json!({
        "id": "build",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {
                "shell": "bash"
            },
            "command": "echo hello"
        }
    }))
    .unwrap();

    let action = node.action.unwrap();
    assert_eq!(action.configuration["shell"], "bash");
    assert_eq!(action.configuration["command"], "echo hello");
}

#[test]
fn workflow_action_rejects_task_metadata_shape() {
    let err = serde_json::from_value::<WorkflowNode>(json!({
        "id": "build",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "metadata": {}
        }
    }))
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("action metadata is no longer supported")
    );
}

#[test]
fn workflow_node_kind_accepts_rich_control_flow_nodes() {
    for (kind, expected) in [
        ("switch", WorkflowNodeKind::Switch),
        ("parallel", WorkflowNodeKind::Parallel),
        ("join", WorkflowNodeKind::Join),
        ("try", WorkflowNodeKind::Try),
        ("map", WorkflowNodeKind::Map),
        ("race", WorkflowNodeKind::Race),
        ("output", WorkflowNodeKind::Output),
        ("deliverable", WorkflowNodeKind::Deliverable),
        ("input", WorkflowNodeKind::Input),
        ("config", WorkflowNodeKind::Config),
    ] {
        let node: WorkflowNode = serde_json::from_value(json!({
            "id": kind,
            "kind": kind,
            "parameters": {}
        }))
        .unwrap();
        assert_eq!(node.kind, expected);
    }
}

#[test]
fn provider_metadata_accepts_catalog_provider_name() {
    let metadata: ProviderMetadata = serde_json::from_value(json!({
        "provider_name": "git",
        "actions": [
            { "function_name": "diff", "description": "Get diff" }
        ]
    }))
    .unwrap();

    assert_eq!(metadata.name, "git");
    assert_eq!(metadata.actions[0].function_name, "diff");
    assert!(metadata.metadata.credential_scopes.is_empty());
}

#[test]
fn provider_metadata_validation_rejects_bad_defaults_and_duplicates() {
    let provider = ProviderMetadata {
        name: "typed".into(),
        actions: vec![ActionMetadata::new("run", "run").with_parameters(vec![
                ParameterMetadata::optional("count", RuninatorType::Integer)
                    .with_default(crate::json!("bad")),
            ])],
        metadata: Default::default(),
    };
    let err = validate_provider_metadata(&provider).unwrap_err();
    assert!(err.contains("provider 'typed.run' parameter 'count' expected integer, got string"));

    let duplicate = ProviderMetadata {
        name: "typed".into(),
        actions: vec![ActionMetadata::new("run", "run").with_parameters(vec![
            ParameterMetadata::required("name", RuninatorType::String),
            ParameterMetadata::optional("name", RuninatorType::String),
        ])],
        metadata: Default::default(),
    };
    let err = validate_provider_metadata(&duplicate).unwrap_err();
    assert!(err.contains("duplicate parameter 'name'"));
}

#[test]
fn action_metadata_exposes_typed_parameter_and_result_environments() {
    let action = ActionMetadata::new("run", "run")
        .with_parameters(vec![
            ParameterMetadata::required("name", RuninatorType::String),
            ParameterMetadata::optional("count", RuninatorType::Integer)
                .with_default(crate::json!(3)),
        ])
        .with_results(vec![ResultMetadata::new(
            "output",
            RuninatorType::typed_structure([
                ("status", RuninatorField::required(RuninatorType::String)),
                ("size", RuninatorField::optional(RuninatorType::Integer)),
            ]),
        )]);

    assert_eq!(
        action.parameters_type(),
        RuninatorType::typed_structure([
            ("name", RuninatorField::required(RuninatorType::String)),
            (
                "count",
                RuninatorField::optional(RuninatorType::Integer).with_default(crate::json!(3)),
            ),
        ])
    );
    assert_eq!(
        action.results_type(),
        RuninatorType::open_typed_structure(
            [(
                "output",
                RuninatorField::optional(RuninatorType::typed_structure([
                    ("status", RuninatorField::required(RuninatorType::String)),
                    ("size", RuninatorField::optional(RuninatorType::Integer)),
                ])),
            )],
            RuninatorType::Any,
        )
    );
}

#[test]
fn runinator_type_round_trips_recursive_shapes() {
    let ty = RuninatorType::typed_structure([
        ("name", RuninatorField::required(RuninatorType::String)),
        (
            "labels",
            RuninatorField::optional(RuninatorType::map(RuninatorType::array(
                RuninatorType::String,
            ))),
        ),
    ]);

    let value = serde_json::to_value(&ty).unwrap();
    assert_eq!(value["type"], "struct");
    assert_eq!(value["fields"]["labels"]["ty"]["type"], "map");
    assert_eq!(value["fields"]["labels"]["required"], false);

    let decoded: RuninatorType = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, ty);
}

#[test]
fn runinator_type_imports_legacy_json_schema() {
    let ty: RuninatorType = serde_json::from_value(json!({
        "type": "object",
        "required": ["items"],
        "properties": {
            "items": {
                "type": "array",
                "items": { "type": "integer" }
            }
        }
    }))
    .unwrap();

    assert_eq!(
        ty,
        RuninatorType::typed_structure([(
            "items",
            RuninatorField::required(RuninatorType::array(RuninatorType::Integer))
        )])
    );
}

#[test]
fn runinator_type_imports_json_schema_edge_shapes() {
    assert_eq!(
        RuninatorType::from_json_schema(&crate::json!({ "oneOf": [
            { "type": "string" },
            { "type": "integer" }
        ] })),
        RuninatorType::Union(vec![RuninatorType::String, RuninatorType::Integer])
    );
    assert_eq!(
        RuninatorType::from_json_schema(
            &crate::json!({ "type": ["array", "null"], "items": { "type": "string" } })
        ),
        RuninatorType::Union(vec![
            RuninatorType::array(RuninatorType::String),
            RuninatorType::Null
        ])
    );
    assert_eq!(
        RuninatorType::from_json_schema(&crate::json!({ "enum": ["open", "closed"] })),
        RuninatorType::String
    );
    assert_eq!(
        RuninatorType::from_json_schema(&crate::json!({ "const": 1 })),
        RuninatorType::Integer
    );
    assert_eq!(
        RuninatorType::from_json_schema(&crate::json!({
            "allOf": [
                {
                    "type": "object",
                    "required": ["name"],
                    "properties": { "name": { "type": "string" } }
                },
                {
                    "type": "object",
                    "properties": { "count": { "type": "integer" } }
                }
            ]
        })),
        RuninatorType::typed_structure([
            ("count", RuninatorField::optional(RuninatorType::Integer)),
            ("name", RuninatorField::required(RuninatorType::String)),
        ])
    );
}

#[test]
fn runinator_type_checked_json_schema_rejects_unsupported_edges() {
    let tuple_items = RuninatorType::from_json_schema_checked(&crate::json!({
        "type": "array",
        "items": [{ "type": "string" }]
    }))
    .unwrap_err();
    assert!(tuple_items.contains("$.items tuple arrays are not supported"));

    let pattern_properties = RuninatorType::from_json_schema_checked(&crate::json!({
        "type": "object",
        "patternProperties": {
            "^x-": { "type": "string" }
        }
    }))
    .unwrap_err();
    assert!(pattern_properties.contains("$.patternProperties is not supported"));

    RuninatorType::from_json_schema_checked(&crate::json!({
        "oneOf": [{ "type": "string" }, { "type": "null" }]
    }))
    .expect("supported oneOf schemas pass checked conversion");
}

#[test]
fn runinator_type_accepts_legacy_schema_field_required_arrays() {
    let ty: RuninatorType = serde_json::from_value(json!({
        "type": "struct",
        "fields": {
            "config": {
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": { "type": "string" }
                }
            }
        }
    }))
    .unwrap();

    assert_eq!(
        ty,
        RuninatorType::typed_structure([(
            "config",
            RuninatorField::required(RuninatorType::typed_structure([(
                "name",
                RuninatorField::required(RuninatorType::String)
            )]))
        )])
    );
}

#[test]
fn runinator_type_validates_recursive_values() {
    let ty = RuninatorType::open_typed_structure(
        [
            ("name", RuninatorField::required(RuninatorType::String)),
            (
                "tags",
                RuninatorField::optional(RuninatorType::array(RuninatorType::String)),
            ),
            (
                "env",
                RuninatorField::required(RuninatorType::map(RuninatorType::String)),
            ),
        ],
        RuninatorType::Union(vec![RuninatorType::String, RuninatorType::Integer]),
    );

    ty.validate_value(&crate::json!({
        "name": "build",
        "env": { "RUST_LOG": "info" },
        "attempt": 1
    }))
    .expect("valid recursive value passes");

    let err = ty
        .validate_value(&crate::json!({
            "name": "build",
            "env": { "RUST_LOG": 1 }
        }))
        .expect_err("nested map type is checked");
    assert_eq!(err.path, "$.env.RUST_LOG");
    assert_eq!(err.expected, "string");
    assert_eq!(err.actual, "integer");

    let missing = ty
        .validate_value(&crate::json!({ "env": {} }))
        .expect_err("required fields are checked");
    assert_eq!(missing.path, "$.name");
    assert_eq!(missing.actual, "missing");
}

#[test]
fn runinator_type_rejects_closed_struct_additional_fields() {
    let ty =
        RuninatorType::typed_structure([("name", RuninatorField::required(RuninatorType::String))]);

    let err = ty
        .validate_value(&crate::json!({ "name": "build", "extra": true }))
        .expect_err("closed struct rejects additional fields");
    assert_eq!(err.path, "$.extra");
    assert_eq!(err.actual, "unexpected");
}

#[test]
fn runinator_type_validates_assignability() {
    RuninatorType::Integer
        .validate_assignable_to(&RuninatorType::Number)
        .expect("integer is assignable to number");
    RuninatorType::array(RuninatorType::Integer)
        .validate_assignable_to(&RuninatorType::array(RuninatorType::Number))
        .expect("array item assignability is recursive");
    RuninatorType::typed_structure([
        ("name", RuninatorField::required(RuninatorType::String)),
        ("count", RuninatorField::required(RuninatorType::Integer)),
    ])
    .validate_assignable_to(&RuninatorType::open_typed_structure(
        [("name", RuninatorField::required(RuninatorType::String))],
        RuninatorType::Number,
    ))
    .expect("struct extra fields are checked through additional type");
    RuninatorType::String
        .validate_assignable_to(&RuninatorType::Union(vec![
            RuninatorType::Integer,
            RuninatorType::String,
        ]))
        .expect("actual type may match one expected union variant");
}

#[test]
fn runinator_type_reports_nested_assignability_paths() {
    let actual = RuninatorType::typed_structure([(
        "env",
        RuninatorField::required(RuninatorType::typed_structure([(
            "API_KEY",
            RuninatorField::required(RuninatorType::Integer),
        )])),
    )]);
    let expected = RuninatorType::typed_structure([(
        "env",
        RuninatorField::required(RuninatorType::typed_structure([(
            "API_KEY",
            RuninatorField::required(RuninatorType::String),
        )])),
    )]);

    let err = actual
        .validate_assignable_to(&expected)
        .expect_err("nested mismatch reports precise path");
    assert_eq!(err.path, "$.env.API_KEY");
    assert_eq!(err.expected, "string");
    assert_eq!(err.actual, "integer");

    let missing =
        RuninatorType::typed_structure([("env", RuninatorField::optional(RuninatorType::String))])
            .validate_assignable_to(&RuninatorType::typed_structure([(
                "env",
                RuninatorField::required(RuninatorType::String),
            )]))
            .expect_err("optional actual field cannot satisfy required expected field");
    assert_eq!(missing.path, "$.env");
    assert_eq!(missing.actual, "missing");
}

#[test]
fn runinator_type_rejects_malformed_field_metadata_and_empty_unions() {
    let malformed_required = serde_json::from_value::<RuninatorType>(json!({
        "type": "struct",
        "fields": {
            "name": {
                "ty": { "type": "string" },
                "required": "sometimes"
            }
        }
    }))
    .unwrap_err();
    assert!(
        malformed_required
            .to_string()
            .contains("field required must be a boolean")
    );

    let legacy_required_without_ty = serde_json::from_value::<RuninatorType>(json!({
        "type": "struct",
        "fields": {
            "name": {
                "type": "string",
                "required": false
            }
        }
    }))
    .unwrap_err();
    assert!(
        legacy_required_without_ty
            .to_string()
            .contains("field required requires field ty")
    );

    let empty_union = serde_json::from_value::<RuninatorType>(json!({
        "type": "union",
        "variants": []
    }))
    .unwrap_err();
    assert!(
        empty_union
            .to_string()
            .contains("union variants must not be empty")
    );
}

#[test]
fn runinator_type_reports_specific_union_validation_errors() {
    let ty = RuninatorType::Union(vec![
        RuninatorType::String,
        RuninatorType::typed_structure([("name", RuninatorField::required(RuninatorType::String))]),
    ]);

    let err = ty
        .validate_value(&crate::json!({ "name": 1 }))
        .expect_err("union reports nested variant error");
    assert_eq!(err.path, "$.name");
    assert_eq!(err.expected, "string");
    assert_eq!(err.actual, "integer");
}

#[test]
fn workflow_bundle_uses_importer_shape() {
    let wf_id = "019ea000-0000-7000-8000-000000000007";
    let trig_id = "019ea000-0000-7000-8000-000000000003";
    let bundle: WorkflowBundle = serde_json::from_value(json!({
        "workflows": [
            {
                "id": wf_id,
                "name": "dev workflow",
                "version": 1,
                "enabled": true,
                "input_type": {},
                "definition": {}
            }
        ],
        "triggers": [
            {
                "id": trig_id,
                "workflow_id": wf_id,
                "kind": "manual",
                "enabled": true,
                "configuration": {},
                "next_execution": null,
                "blackout_start": null,
                "blackout_end": null,
                "metadata": {}
            }
        ]
    }))
    .unwrap();

    assert_eq!(
        bundle.workflows[0].id,
        Some(wf_id.parse::<uuid::Uuid>().unwrap())
    );
    assert_eq!(
        bundle.triggers[0].workflow_id,
        wf_id.parse::<uuid::Uuid>().unwrap()
    );

    let value = serde_json::to_value(bundle).unwrap();
    assert!(value.get("workflows").is_some());
    assert!(value.get("triggers").is_some());
}

#[test]
fn retry_class_selects_retryable_statuses() {
    assert!(WorkflowRetryClass::Any.retryable(WorkflowStatus::Failed));
    assert!(WorkflowRetryClass::Any.retryable(WorkflowStatus::TimedOut));
    assert!(!WorkflowRetryClass::Any.retryable(WorkflowStatus::Succeeded));

    assert!(WorkflowRetryClass::Failure.retryable(WorkflowStatus::Failed));
    assert!(!WorkflowRetryClass::Failure.retryable(WorkflowStatus::TimedOut));

    assert!(WorkflowRetryClass::Timeout.retryable(WorkflowStatus::TimedOut));
    assert!(!WorkflowRetryClass::Timeout.retryable(WorkflowStatus::Failed));
}

#[test]
fn retry_defaults_preserve_legacy_shape() {
    // a bare `{ "max_attempts": 3 }` must still deserialize, defaulting the new fields.
    let retry: WorkflowRetry = serde_json::from_value(json!({ "max_attempts": 3 })).unwrap();
    assert_eq!(retry.max_attempts, 3);
    assert_eq!(retry.backoff_base_seconds, 1);
    assert_eq!(retry.backoff_max_seconds, 300);
    assert!(!retry.jitter);
    assert_eq!(retry.retry_on, WorkflowRetryClass::Any);
}
