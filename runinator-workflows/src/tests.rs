use super::*;
use runinator_models::{
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    types::RuninatorField,
    workflows::{
        WorkflowDefinition, WorkflowGraph, WorkflowNode, WorkflowNodeKind, WorkflowStatus,
    },
};
use std::collections::HashMap;

fn workflow(definition: runinator_models::value::Value) -> WorkflowDefinition {
    WorkflowDefinition {
        id: Some(1),
        name: "test".into(),
        version: 1,
        enabled: true,
        input_type: RuninatorType::Any,
        definition: WorkflowGraph::from_value(definition).unwrap(),
        created_at: None,
        updated_at: None,
    }
}

#[test]
fn validates_state_machine_workflow() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} }, "transitions": { "on_success": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("rich control flow validates");
}

#[test]
fn rejects_missing_transition_target() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            { "id": "done", "kind": "end" },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} }, "transitions": { "on_success": { "$node": "missing" } } }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MissingTransition { .. })
    ));
}

#[test]
fn validates_subflow_target_by_id_or_name() {
    let named = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "spawn" } } },
            {
                "id": "spawn",
                "kind": "subflow",
                "subflow": { "workflow_name": "Ticket Work", "type": "fire_and_forget" },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));
    validate_workflow(&named).expect("named subflow target validates");

    let missing = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "spawn" } } },
            { "id": "spawn", "kind": "subflow", "transitions": { "on_success": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(matches!(
        validate_workflow(&missing),
        Err(WorkflowValidationError::MissingSubflowTarget(_))
    ));
}

#[test]
fn rejects_old_reference_syntax() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": "build" } },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} } },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidNode(_))
    ));

    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            { "id": "build", "kind": "emit", "parameters": { "data": { "$value": "input#/value" } }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidValueRef(_))
    ));
}

#[test]
fn resolves_value_refs() {
    let context = runinator_models::json!({
        "steps": { "find": { "output": { "items": [{ "key": "A-1" }] } } }
    });
    let value =
        runinator_models::json!({ "$ref": { "node": "find", "output": ["items", 0, "key"] } });
    assert_eq!(
        resolve_value_refs(&value, &context).unwrap(),
        runinator_models::value::Value::String("A-1".into())
    );
}

#[test]
fn accepts_structurally_valid_refs_without_schema_path_validation() {
    let wf = WorkflowDefinition {
        id: Some(1),
        name: "schema-boundary".into(),
        version: 1,
        enabled: true,
        input_type: RuninatorType::from_json_schema(&runinator_models::json!({
            "type": "object",
            "properties": {
                "known": { "type": "string" }
            }
        })),
        definition: WorkflowGraph::from_value(runinator_models::json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "produce" } } },
                {
                    "id": "produce",
                    "kind": "emit",
                    "parameters": {
                        "data": { "ok": true }
                    },
                    "transitions": { "next": { "$node": "consume" } }
                },
                {
                    "id": "consume",
                    "kind": "emit",
                    "parameters": {
                        "data": {
                            "input": { "$ref": { "input": ["not_in_input_type"] } },
                            "output": { "$ref": { "node": "produce", "output": ["not_in_result_metadata"] } }
                        }
                    },
                    "transitions": { "next": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    };

    validate_workflow(&wf).expect("schema path validation is out of scope");
}

#[test]
fn resolves_template_refs() {
    let context = runinator_models::json!({
        "prev": { "ticket_id": "RUN-123", "count": 3 }
    });

    assert_eq!(
        resolve_value_refs(
            &runinator_models::json!({ "$concat": ["Ticket ", { "$ref": { "prev": ["ticket_id"] } }] }),
            &context
        )
        .unwrap(),
        runinator_models::value::Value::String("Ticket RUN-123".into())
    );
    assert_eq!(
        resolve_value_refs(
            &runinator_models::json!({ "$ref": { "prev": ["count"] } }),
            &context
        )
        .unwrap(),
        runinator_models::value::Value::from(3)
    );
}

#[test]
fn expands_local_defs_with_overlay() {
    let wf = workflow(runinator_models::json!({
        "$defs": {
            "approval": { "kind": "approval", "parameters": { "type": "merge" } }
        },
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "approve" } } },
            { "id": "approve", "$ref": "#/$defs/approval", "with": { "parameters": { "prompt": "ok?" } }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    let (_, nodes) = parse_nodes(&wf).unwrap();
    assert_eq!(nodes[1].kind, WorkflowNodeKind::Approval);
    assert_eq!(nodes[1].parameters["type"], "merge");
    assert_eq!(nodes[1].parameters["prompt"], "ok?");
}

#[test]
fn evaluates_conditions() {
    let context = runinator_models::json!({
        "input": { "env": "prod" },
        "steps": { "check": { "output": { "status": "ok", "count": 10 } } }
    });

    // simple equality.
    let cond1 = runinator_models::json!({ "value": { "$ref": { "node": "check", "output": ["status"] } }, "equals": "ok" });
    assert!(evaluate_condition(&cond1, &context).unwrap());

    // logical all (and).
    let cond3 = runinator_models::json!({
        "all": [
            { "value": { "$ref": { "input": ["env"] } }, "equals": "prod" },
            { "value": { "$ref": { "node": "check", "output": ["status"] } }, "equals": "ok" }
        ]
    });
    assert!(evaluate_condition(&cond3, &context).unwrap());

    // logical any (or).
    let cond4 = runinator_models::json!({
        "any": [
            { "value": { "$ref": { "input": ["env"] } }, "equals": "dev" },
            { "value": { "$ref": { "node": "check", "output": ["count"] } }, "equals": 10 }
        ]
    });
    assert!(evaluate_condition(&cond4, &context).unwrap());
}

#[test]
fn evaluates_richer_conditions() {
    let context = runinator_models::json!({
        "input": {
            "ticket": "ITP-123",
            "labels": ["auto-implement", "backend"],
            "fields": { "priority": "high" },
            "score": 7
        }
    });

    for condition in [
        runinator_models::json!({ "value": { "$ref": { "input": ["ticket"] } }, "starts_with": "ITP-" }),
        runinator_models::json!({ "value": { "$ref": { "input": ["ticket"] } }, "ends_with": "123" }),
        runinator_models::json!({ "value": { "$ref": { "input": ["ticket"] } }, "contains": "TP-1" }),
        runinator_models::json!({ "value": { "$ref": { "input": ["labels"] } }, "contains": "auto-implement" }),
        runinator_models::json!({ "value": { "$ref": { "input": ["fields"] } }, "contains": "priority" }),
        runinator_models::json!({ "value": { "$ref": { "input": ["ticket"] } }, "in": ["OPS-1", "ITP-123"] }),
        runinator_models::json!({ "value": { "$ref": { "input": ["score"] } }, "greater_than": 5 }),
        runinator_models::json!({ "value": { "$ref": { "input": ["score"] } }, "less_than_or_equal": 7 }),
    ] {
        assert!(
            evaluate_condition(&condition, &context).unwrap(),
            "{condition}"
        );
    }
}

#[test]
fn validates_node_transitions() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "a" } } },
            {
                "id": "a",
                "kind": "condition",
                "transitions": {
                    "branches": [{ "when": { "value": { "$ref": { "input": ["foo"] } }, "equals": "bar" }, "target": { "$node": "b" } }],
                    "next": { "$node": "c" }
                }
            },
            { "id": "b", "kind": "end" },
            { "id": "c", "kind": "end" }
        ]
    }));
    validate_workflow(&wf).expect("rich control flow validates");
}

#[test]
fn validates_rich_control_flow_node_targets() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "route" } } },
            {
                "id": "route",
                "kind": "switch",
                "parameters": {
                    "value": { "$ref": { "input": ["mode"] } },
                    "cases": [
                        { "equals": "fanout", "target": { "$node": "fanout" } },
                        { "equals": "batch", "target": { "$node": "batch" } }
                    ],
                    "default": { "$node": "emit" }
                }
            },
            { "id": "fanout", "kind": "parallel", "parameters": { "branches": [{ "$node": "check_a" }, { "$node": "check_b" }] } },
            { "id": "check_a", "kind": "emit", "parameters": { "data": { "check": "a" } }, "transitions": { "next": { "$node": "joined" } } },
            { "id": "check_b", "kind": "emit", "parameters": { "data": { "check": "b" } }, "transitions": { "next": { "$node": "joined" } } },
            { "id": "joined", "kind": "join", "parameters": { "wait_for": [{ "$node": "check_a" }, { "$node": "check_b" }], "mode": "all" }, "transitions": { "next": { "$node": "guarded" } } },
            { "id": "guarded", "kind": "try", "parameters": { "body": { "$node": "body" }, "catch": { "$node": "catch" }, "finally": { "$node": "finally" } }, "transitions": { "next": { "$node": "done" } } },
            { "id": "body", "kind": "emit", "parameters": { "data": "body" }, "transitions": { "next": { "$node": "guarded" } } },
            { "id": "catch", "kind": "emit", "parameters": { "data": "catch" }, "transitions": { "next": { "$node": "guarded" } } },
            { "id": "finally", "kind": "emit", "parameters": { "data": "finally" }, "transitions": { "next": { "$node": "guarded" } } },
            { "id": "batch", "kind": "map", "parameters": { "items": [1, 2], "target": { "$node": "map_item" }, "concurrency": 1 }, "transitions": { "next": { "$node": "race" } } },
            { "id": "map_item", "kind": "emit", "parameters": { "data": { "$ref": { "workflow": ["state", "map", "item"] } } }, "transitions": { "next": { "$node": "batch" } } },
            { "id": "race", "kind": "race", "parameters": { "branches": [{ "$node": "fast" }, { "$node": "slow" }], "winner": "first_success" }, "transitions": { "next": { "$node": "done" } } },
            { "id": "fast", "kind": "emit", "parameters": { "data": "fast" }, "transitions": { "next": { "$node": "race" } } },
            { "id": "slow", "kind": "emit", "parameters": { "data": "slow" }, "transitions": { "next": { "$node": "race" } } },
            { "id": "emit", "kind": "emit", "parameters": { "event_type": "workflow.routed", "data": { "ok": true } }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("rich control flow validates");
}

#[test]
fn rejects_missing_control_flow_target() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "route" } } },
            {
                "id": "route",
                "kind": "switch",
                "parameters": {
                    "value": "mode",
                    "cases": [{ "equals": "missing", "target": { "$node": "missing" } }]
                }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MissingTransition { .. })
    ));
}

#[test]
fn rejects_invalid_map_concurrency() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "batch" } } },
            { "id": "batch", "kind": "map", "parameters": { "items": [], "target": { "$node": "item" }, "concurrency": 0 } },
            { "id": "item", "kind": "emit", "parameters": { "data": null }, "transitions": { "next": { "$node": "batch" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidNodeParameters { .. })
    ));
}

#[test]
fn validates_loop_body_returning_to_loop_node() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "for_each_ticket" } } },
            {
                "id": "for_each_ticket",
                "kind": "loop",
                "parameters": {
                    "items": { "$ref": { "input": ["tickets"] } }
                },
                "max_iterations": 50,
                "transitions": {
                    "next": { "$node": "process_ticket" },
                    "on_success": { "$node": "done" }
                }
            },
            {
                "id": "process_ticket",
                "kind": "emit",
                "parameters": {
                    "data": { "$ref": { "node": "for_each_ticket", "output": ["item", "key"] } }
                },
                "transitions": { "next": { "$node": "for_each_ticket" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("loop body can return to loop node");
}

#[test]
fn validates_explicit_bounded_reentry_cycle() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            {
                "id": "build",
                "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
                "reentry": { "enabled": true, "max_visits": 3, "on_exhausted": { "$node": "deferred" } },
                "transitions": { "on_success": { "$node": "review" } }
            },
            { "id": "review", "kind": "approval", "transitions": { "on_success": { "$node": "done" }, "on_failure": { "$node": "build" } } },
            { "id": "deferred", "kind": "end" },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("bounded reentry cycle validates");
}

#[test]
fn rejects_unbounded_reentry_cycle() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} }, "transitions": { "on_success": { "$node": "review" } } },
            { "id": "review", "kind": "approval", "transitions": { "on_failure": { "$node": "build" }, "on_success": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::RefCycle(_))
    ));
}

#[test]
fn rejects_invalid_reentry_configuration() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            {
                "id": "build",
                "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
                "reentry": { "enabled": true, "max_visits": 0 },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidReentry(_))
    ));

    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            {
                "id": "build",
                "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
                "reentry": { "enabled": true, "max_visits": 2, "on_exhausted": { "$node": "missing" } },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MissingTransition { .. })
    ));
}

#[test]
fn evaluates_switch_cases_and_default() {
    let node: WorkflowNode = serde_json::from_value(
        runinator_models::json!({
            "id": "route",
            "kind": "switch",
            "parameters": {
                "value": { "$ref": { "input": ["mode"] } },
                "cases": [
                    { "equals": "fast", "target": { "$node": "fast_path" } },
                    { "equals": "slow", "target": { "$node": "slow_path" } }
                ],
                "default": { "$node": "fallback" }
            }
        })
        .into(),
    )
    .unwrap();
    let params = parse_switch_parameters(&node).unwrap();

    assert_eq!(
        evaluate_switch(
            &params,
            &runinator_models::json!({ "input": { "mode": "slow" } })
        )
        .unwrap(),
        Some("slow_path".into())
    );
    assert_eq!(
        evaluate_switch(
            &params,
            &runinator_models::json!({ "input": { "mode": "other" } })
        )
        .unwrap(),
        Some("fallback".into())
    );
}

#[test]
fn test_workflow_state_machine_logic_integration() {
    // 1. define a simple state-machine workflow.
    let definition = runinator_models::json!({
        "start": "start",
        "nodes": [
            {
                "id": "start",
                "kind": "start",
                "transitions": { "next": { "$node": "step1" } }
            },
            {
                "id": "step1",
                "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
                "transitions": { "on_success": { "$node": "step2" }, "on_failure": { "$node": "failed" } }
            },
            {
                "id": "step2",
                "kind": "condition",
                "transitions": {
                    "branches": [{ "when": { "value": { "$ref": { "node": "step1", "output": ["ok"] } }, "equals": true }, "target": { "$node": "success" } }],
                    "next": { "$node": "failed" }
                }
            },
            { "id": "success", "kind": "end" },
            { "id": "failed", "kind": "end" }
        ]
    });

    let wf = WorkflowDefinition {
        id: Some(1),
        name: "integration-test".into(),
        version: 1,
        enabled: true,
        input_type: RuninatorType::Any,
        definition: WorkflowGraph::from_value(definition.clone()).unwrap(),
        created_at: None,
        updated_at: None,
    };

    // 2. validate the workflow.
    let (start, nodes) = validate_workflow(&wf).expect("Workflow should be valid");
    assert_eq!(start, "start");
    let node_map: HashMap<String, &WorkflowNode> =
        nodes.iter().map(|n| (n.id.clone(), n)).collect();

    // 3. simulate execution - step 1 succeeds.
    let step1_node = node_map.get("step1").unwrap();
    let next = next_transition(
        step1_node,
        WorkflowStatus::Succeeded,
        &runinator_models::json!({}),
    )
    .unwrap();
    assert_eq!(next.unwrap(), "step2");

    // 4. simulate step 2 - condition evaluation.
    let outputs = {
        let mut m = HashMap::new();
        m.insert("step1".to_string(), runinator_models::json!({ "ok": true }));
        m
    };
    let context = outputs_context(&runinator_models::json!({}), &outputs);

    let step2_node = node_map.get("step2").unwrap();
    let next = next_transition(step2_node, WorkflowStatus::Running, &context).unwrap();
    assert_eq!(next.unwrap(), "success");

    // 5. simulate step 2 - condition failure.
    let outputs_fail = {
        let mut m = HashMap::new();
        m.insert(
            "step1".to_string(),
            runinator_models::json!({ "ok": false }),
        );
        m
    };
    let context_fail = outputs_context(&runinator_models::json!({}), &outputs_fail);
    let next_fail = next_transition(step2_node, WorkflowStatus::Running, &context_fail).unwrap();
    assert_eq!(next_fail.unwrap(), "failed");
}

#[test]
fn normalizes_legacy_workflow_with_start_and_end_nodes() {
    let wf = workflow(runinator_models::json!({
        "start": "build",
        "nodes": [
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} }, "transitions": {} }
        ],
        "ui": {
            "layout": {
                "build": { "x": 10, "y": 20 }
            }
        }
    }));

    let normalized = normalize_workflow(&wf);
    let definition = normalized.definition.as_value();
    let definition = definition.as_object().unwrap();
    assert_eq!(definition["start"], "start");
    assert_eq!(definition["ui"]["layout"]["nodes"]["build"]["x"], 10);
    let (_, nodes) = validate_workflow(&normalized).expect("normalized workflow is valid");
    assert!(nodes
        .iter()
        .any(|node| node.kind == WorkflowNodeKind::Start));
    assert!(nodes.iter().any(|node| node.kind == WorkflowNodeKind::End));
    assert!(nodes.iter().any(|node| node.kind == WorkflowNodeKind::Fail));
    let build = nodes.iter().find(|node| node.id == "build").unwrap();
    assert_eq!(
        build
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str()),
        Some("end")
    );
}

fn typed_provider() -> ProviderMetadata {
    ProviderMetadata {
        name: "typed".into(),
        actions: vec![ActionMetadata::new("make", "make typed output")
            .with_parameters(vec![ParameterMetadata::required(
                "name",
                RuninatorType::String,
            )])
            .with_results(vec![
                ResultMetadata::new("count", RuninatorType::Integer),
                ResultMetadata::new("payload", RuninatorType::Any),
                ResultMetadata::new(
                    "items",
                    RuninatorType::array(RuninatorType::structure([(
                        "key",
                        RuninatorType::String,
                    )])),
                ),
            ])],
        metadata: ProviderRuntimeMetadata::default(),
    }
}

fn typed_workflow(
    input_type: RuninatorType,
    node: runinator_models::value::Value,
) -> WorkflowDefinition {
    let mut wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "make" } } },
            {
                "id": "make",
                "kind": "action",
                "action": {
                    "provider": "typed",
                    "function": "make",
                    "configuration": { "name": { "$ref": { "input": ["name"] } } }
                },
                "transitions": { "on_success": { "$node": "checked" } }
            },
            node,
            { "id": "done", "kind": "end" }
        ]
    }));
    wf.input_type = input_type;
    wf
}

fn schema_type(schema: runinator_models::value::Value) -> RuninatorType {
    RuninatorType::from_json_schema(&schema)
}

#[test]
fn typed_validation_requires_known_input_paths() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": { "name": { "$ref": { "input": ["missing"] } } },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

#[test]
fn typed_validation_rejects_implicit_concat_coercion() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": {
                "name": {
                    "$concat": [
                        "count:",
                        { "$ref": { "node": "make", "output": ["count"] } }
                    ]
                }
            },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

#[test]
fn typed_validation_accepts_explicit_string_conversions() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": {
                "name": {
                    "$concat": [
                        "count:",
                        { "$to_string": { "$ref": { "node": "make", "output": ["count"] } } },
                        " json:",
                        { "$to_json_string": { "$ref": { "node": "make", "output": ["items"] } } }
                    ]
                }
            },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    validate_workflow_with_providers(&wf, &[typed_provider()])
        .expect("explicit conversions validate");
}

#[test]
fn typed_validation_rejects_opaque_json_traversal() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": {
                "name": { "$ref": { "node": "make", "output": ["payload", "key"] } }
            },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

#[test]
fn typed_validation_checks_action_parameter_types() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "integer" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": { "name": "done" },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

#[test]
fn typed_validation_requires_map_items_to_be_array() {
    let mut wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
            {
                "id": "map",
                "kind": "map",
                "parameters": {
                    "items": { "$ref": { "input": ["name"] } },
                    "target": { "$node": "done" }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));
    wf.input_type = RuninatorType::from_json_schema(&runinator_models::json!({
        "type": "object",
        "properties": { "name": { "type": "string" } }
    }));

    assert!(validate_workflow_with_providers(&wf, &[]).is_err());
}

fn action_workflow(configuration: runinator_models::value::Value) -> WorkflowDefinition {
    workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "check" } } },
            {
                "id": "check",
                "kind": "action",
                "action": {
                    "provider": "typed",
                    "function": "check",
                    "configuration": configuration
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
}

fn check_provider(param_type: RuninatorType) -> ProviderMetadata {
    ProviderMetadata {
        name: "typed".into(),
        actions: vec![ActionMetadata::new("check", "check typed input")
            .with_parameters(vec![ParameterMetadata::required("config", param_type)])],
        metadata: ProviderRuntimeMetadata::default(),
    }
}

#[test]
fn typed_validation_rejects_provider_default_value_mismatch() {
    let provider = ProviderMetadata {
        name: "typed".into(),
        actions: vec![
            ActionMetadata::new("check", "check typed input").with_parameters(vec![
                ParameterMetadata::optional("count", RuninatorType::Integer)
                    .with_default(runinator_models::json!("bad")),
            ]),
        ],
        metadata: ProviderRuntimeMetadata::default(),
    };
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(err
        .to_string()
        .contains("provider 'typed.check' parameter 'count' expected integer, got string"));
}

#[test]
fn typed_validation_reports_missing_required_nested_literal_field() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "env",
        RuninatorField::required(RuninatorType::typed_structure([(
            "API_KEY",
            RuninatorField::required(RuninatorType::String),
        )])),
    )]));
    let wf = action_workflow(runinator_models::json!({
        "config": { "env": {} }
    }));

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(err
        .to_string()
        .contains("action parameter 'config.env.API_KEY' is missing required field"));
    let diagnostic = err
        .type_diagnostic()
        .expect("type diagnostic is structured");
    assert_eq!(diagnostic.path, "action parameter 'config.env.API_KEY'");
    assert_eq!(diagnostic.expected, "string");
    assert_eq!(diagnostic.actual, "missing");
}

#[test]
fn typed_validation_accepts_absent_optional_literal_field() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "env",
        RuninatorField::optional(RuninatorType::typed_structure([(
            "API_KEY",
            RuninatorField::required(RuninatorType::String),
        )])),
    )]));
    let wf = action_workflow(runinator_models::json!({ "config": {} }));

    validate_workflow_with_providers(&wf, &[provider]).expect("optional field may be absent");
}

#[test]
fn typed_validation_rejects_closed_struct_additional_literal_fields() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "name",
        RuninatorField::required(RuninatorType::String),
    )]));
    let wf = action_workflow(runinator_models::json!({
        "config": { "name": "build", "extra": true }
    }));

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(err
        .to_string()
        .contains("action parameter 'config.extra' is not allowed"));
}

#[test]
fn typed_validation_validates_open_struct_additional_literal_fields() {
    let provider = check_provider(RuninatorType::open_typed_structure(
        [("name", RuninatorField::required(RuninatorType::String))],
        RuninatorType::String,
    ));
    let valid = action_workflow(runinator_models::json!({
        "config": { "name": "build", "extra": "ok" }
    }));
    validate_workflow_with_providers(&valid, std::slice::from_ref(&provider))
        .expect("open struct validates additional field values");

    let invalid = action_workflow(runinator_models::json!({
        "config": { "name": "build", "extra": 1 }
    }));
    let err = validate_workflow_with_providers(&invalid, &[provider]).unwrap_err();
    assert!(err
        .to_string()
        .contains("action parameter 'config.extra' expected string, got integer"));
}

#[test]
fn typed_validation_reports_nested_literal_errors_inside_dynamic_configs() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "env",
        RuninatorField::required(RuninatorType::typed_structure([
            ("API_KEY", RuninatorField::required(RuninatorType::String)),
            ("TOKEN", RuninatorField::required(RuninatorType::String)),
        ])),
    )]));
    let mut wf = action_workflow(runinator_models::json!({
        "config": {
            "env": {
                "API_KEY": 1,
                "TOKEN": { "$ref": { "input": ["token"] } }
            }
        }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "token",
        RuninatorField::required(RuninatorType::String),
    )]);

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(err
        .to_string()
        .contains("action parameter 'config.env.API_KEY' expected string, got integer"));
}

#[test]
fn typed_validation_reports_nested_dynamic_expression_type_errors() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "branch",
        RuninatorField::required(RuninatorType::String),
    )]));
    let mut wf = action_workflow(runinator_models::json!({
        "config": {
            "branch": { "$ref": { "input": ["count"] } }
        }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "count",
        RuninatorField::required(RuninatorType::Integer),
    )]);

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(err
        .to_string()
        .contains("action parameter 'config.branch' expected string, got integer"));
}

#[test]
fn typed_validation_keeps_optional_field_refs_presence_only() {
    let provider = check_provider(RuninatorType::String);
    let mut wf = action_workflow(runinator_models::json!({
        "config": { "$ref": { "input": ["maybe_name"] } }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "maybe_name",
        RuninatorField::optional(RuninatorType::String),
    )]);

    validate_workflow_with_providers(&wf, &[provider])
        .expect("optional refs resolve as their declared type");
}

#[test]
fn typed_validation_accepts_explicit_coalesce_defaults() {
    let provider = check_provider(RuninatorType::String);
    let mut wf = action_workflow(runinator_models::json!({
        "config": {
            "$coalesce": [
                { "$ref": { "input": ["maybe_name"] } },
                "fallback"
            ]
        }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "maybe_name",
        RuninatorField::optional(RuninatorType::String),
    )]);

    validate_workflow_with_providers(&wf, &[provider])
        .expect("coalesce resolves to the fallback-compatible type");
}
