use super::*;
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowStatus,
};
use std::collections::HashMap;

fn workflow(definition: serde_json::Value) -> WorkflowDefinition {
    WorkflowDefinition {
        id: Some(1),
        name: "test".into(),
        version: 1,
        enabled: true,
        input_schema: serde_json::Value::Null,
        definition,
        created_at: None,
        updated_at: None,
    }
}

#[test]
fn validates_state_machine_workflow() {
    let wf = workflow(serde_json::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "default_parameters": {} }, "transitions": { "on_success": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("rich control flow validates");
}

#[test]
fn rejects_missing_transition_target() {
    let wf = workflow(serde_json::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            { "id": "done", "kind": "end" },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "default_parameters": {} }, "transitions": { "on_success": { "$node": "missing" } } }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MissingTransition { .. })
    ));
}

#[test]
fn rejects_old_reference_syntax() {
    let wf = workflow(serde_json::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": "build" } },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "default_parameters": {} } },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidNode(_))
    ));

    let wf = workflow(serde_json::json!({
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
    let context = serde_json::json!({
        "steps": { "find": { "output": { "items": [{ "key": "A-1" }] } } }
    });
    let value = serde_json::json!({ "$ref": { "node": "find", "output": ["items", 0, "key"] } });
    assert_eq!(
        resolve_value_refs(&value, &context).unwrap(),
        serde_json::Value::String("A-1".into())
    );
}

#[test]
fn accepts_structurally_valid_refs_without_schema_path_validation() {
    let wf = WorkflowDefinition {
        id: Some(1),
        name: "schema-boundary".into(),
        version: 1,
        enabled: true,
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "known": { "type": "string" }
            }
        }),
        definition: serde_json::json!({
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
                            "input": { "$ref": { "input": ["not_in_input_schema"] } },
                            "output": { "$ref": { "node": "produce", "output": ["not_in_result_metadata"] } }
                        }
                    },
                    "transitions": { "next": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
        created_at: None,
        updated_at: None,
    };

    validate_workflow(&wf).expect("schema path validation is out of scope");
}

#[test]
fn resolves_template_refs() {
    let context = serde_json::json!({
        "prev": { "ticket_id": "RUN-123", "count": 3 }
    });

    assert_eq!(
        resolve_value_refs(
            &serde_json::json!({ "$concat": ["Ticket ", { "$ref": { "prev": ["ticket_id"] } }] }),
            &context
        )
        .unwrap(),
        serde_json::Value::String("Ticket RUN-123".into())
    );
    assert_eq!(
        resolve_value_refs(
            &serde_json::json!({ "$ref": { "prev": ["count"] } }),
            &context
        )
        .unwrap(),
        serde_json::Value::from(3)
    );
}

#[test]
fn expands_local_defs_with_overlay() {
    let wf = workflow(serde_json::json!({
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
    let context = serde_json::json!({
        "input": { "env": "prod" },
        "steps": { "check": { "output": { "status": "ok", "count": 10 } } }
    });

    // Simple equality
    let cond1 = serde_json::json!({ "value": { "$ref": { "node": "check", "output": ["status"] } }, "equals": "ok" });
    assert!(evaluate_condition(&cond1, &context).unwrap());

    // Logical ALL (AND)
    let cond3 = serde_json::json!({
        "all": [
            { "value": { "$ref": { "input": ["env"] } }, "equals": "prod" },
            { "value": { "$ref": { "node": "check", "output": ["status"] } }, "equals": "ok" }
        ]
    });
    assert!(evaluate_condition(&cond3, &context).unwrap());

    // Logical ANY (OR)
    let cond4 = serde_json::json!({
        "any": [
            { "value": { "$ref": { "input": ["env"] } }, "equals": "dev" },
            { "value": { "$ref": { "node": "check", "output": ["count"] } }, "equals": 10 }
        ]
    });
    assert!(evaluate_condition(&cond4, &context).unwrap());
}

#[test]
fn validates_node_transitions() {
    let wf = workflow(serde_json::json!({
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
    let wf = workflow(serde_json::json!({
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
    let wf = workflow(serde_json::json!({
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
    let wf = workflow(serde_json::json!({
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
fn validates_explicit_bounded_reentry_cycle() {
    let wf = workflow(serde_json::json!({
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
            "default_parameters": {}
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
    let wf = workflow(serde_json::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "default_parameters": {} }, "transitions": { "on_success": { "$node": "review" } } },
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
    let wf = workflow(serde_json::json!({
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
            "default_parameters": {}
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

    let wf = workflow(serde_json::json!({
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
            "default_parameters": {}
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
    let node: WorkflowNode = serde_json::from_value(serde_json::json!({
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
    }))
    .unwrap();
    let params = parse_switch_parameters(&node).unwrap();

    assert_eq!(
        evaluate_switch(&params, &serde_json::json!({ "input": { "mode": "slow" } })).unwrap(),
        Some("slow_path".into())
    );
    assert_eq!(
        evaluate_switch(
            &params,
            &serde_json::json!({ "input": { "mode": "other" } })
        )
        .unwrap(),
        Some("fallback".into())
    );
}

#[test]
fn test_workflow_state_machine_logic_integration() {
    // 1. Define a simple state-machine workflow
    let definition = serde_json::json!({
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
            "default_parameters": {}
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
        input_schema: serde_json::json!({}),
        definition: definition.clone(),
        created_at: None,
        updated_at: None,
    };

    // 2. Validate the workflow
    let (start, nodes) = validate_workflow(&wf).expect("Workflow should be valid");
    assert_eq!(start, "start");
    let node_map: HashMap<String, &WorkflowNode> =
        nodes.iter().map(|n| (n.id.clone(), n)).collect();

    // 3. Simulate execution - Step 1 succeeds
    let step1_node = node_map.get("step1").unwrap();
    let next = next_transition(
        step1_node,
        WorkflowStatus::Succeeded,
        &serde_json::json!({}),
    )
    .unwrap();
    assert_eq!(next.unwrap(), "step2");

    // 4. Simulate Step 2 - Condition evaluation
    let outputs = {
        let mut m = HashMap::new();
        m.insert("step1".to_string(), serde_json::json!({ "ok": true }));
        m
    };
    let context = outputs_context(&serde_json::json!({}), &outputs);

    let step2_node = node_map.get("step2").unwrap();
    let next = next_transition(step2_node, WorkflowStatus::Running, &context).unwrap();
    assert_eq!(next.unwrap(), "success");

    // 5. Simulate Step 2 - Condition failure
    let outputs_fail = {
        let mut m = HashMap::new();
        m.insert("step1".to_string(), serde_json::json!({ "ok": false }));
        m
    };
    let context_fail = outputs_context(&serde_json::json!({}), &outputs_fail);
    let next_fail = next_transition(step2_node, WorkflowStatus::Running, &context_fail).unwrap();
    assert_eq!(next_fail.unwrap(), "failed");
}

#[test]
fn normalizes_legacy_workflow_with_start_and_end_nodes() {
    let wf = workflow(serde_json::json!({
        "start": "build",
        "nodes": [
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "default_parameters": {} }, "transitions": {} }
        ],
        "ui": {
            "layout": {
                "build": { "x": 10, "y": 20 }
            }
        }
    }));

    let normalized = normalize_workflow(&wf);
    let definition = normalized.definition.as_object().unwrap();
    assert_eq!(definition["start"], "start");
    assert_eq!(definition["ui"]["layout"]["nodes"]["build"]["x"], 10);
    let (_, nodes) = validate_workflow(&normalized).expect("normalized workflow is valid");
    assert!(
        nodes
            .iter()
            .any(|node| node.kind == WorkflowNodeKind::Start)
    );
    assert!(nodes.iter().any(|node| node.kind == WorkflowNodeKind::End));
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
