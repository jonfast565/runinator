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
            { "id": "start", "kind": "start", "transitions": { "next": "build" } },
            { "id": "build", "kind": "task", "task_id": 1, "transitions": { "on_success": "done" } },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(validate_workflow(&wf).is_ok());
}

#[test]
fn rejects_missing_transition_target() {
    let wf = workflow(serde_json::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": "build" } },
            { "id": "done", "kind": "end" },
            { "id": "build", "kind": "task", "task_id": 1, "transitions": { "on_success": "missing" } }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MissingTransition { .. })
    ));
}

#[test]
fn resolves_value_refs() {
    let context = serde_json::json!({
        "steps": { "find": { "output": { "items": [{ "key": "A-1" }] } } }
    });
    let value = serde_json::json!({ "$value": "steps.find.output#/items/0/key" });
    assert_eq!(
        resolve_value_refs(&value, &context).unwrap(),
        serde_json::Value::String("A-1".into())
    );
}

#[test]
fn resolves_template_refs() {
    let context = serde_json::json!({
        "prev": { "ticket_id": "RUN-123", "count": 3 }
    });

    assert_eq!(
        resolve_value_refs(&serde_json::json!("Ticket {{ prev#/ticket_id }}"), &context).unwrap(),
        serde_json::Value::String("Ticket RUN-123".into())
    );
    assert_eq!(
        resolve_value_refs(&serde_json::json!("{{ prev#/count }}"), &context).unwrap(),
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
            { "id": "start", "kind": "start", "transitions": { "next": "approve" } },
            { "id": "approve", "$ref": "#/$defs/approval", "with": { "parameters": { "prompt": "ok?" } }, "transitions": { "next": "done" } },
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
    let cond1 =
        serde_json::json!({ "value": { "$value": "steps.check.output#/status" }, "equals": "ok" });
    assert!(evaluate_condition(&cond1, &context).unwrap());

    // Logical ALL (AND)
    let cond3 = serde_json::json!({
        "all": [
            { "value": { "$value": "input#/env" }, "equals": "prod" },
            { "value": { "$value": "steps.check.output#/status" }, "equals": "ok" }
        ]
    });
    assert!(evaluate_condition(&cond3, &context).unwrap());

    // Logical ANY (OR)
    let cond4 = serde_json::json!({
        "any": [
            { "value": { "$value": "input#/env" }, "equals": "dev" },
            { "value": { "$value": "steps.check.output#/count" }, "equals": 10 }
        ]
    });
    assert!(evaluate_condition(&cond4, &context).unwrap());
}

#[test]
fn validates_node_transitions() {
    let wf = workflow(serde_json::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": "a" } },
            {
                "id": "a",
                "kind": "condition",
                "transitions": {
                    "branches": [
                        { "when": { "value": { "$value": "foo" }, "equals": "bar" }, "target": "b" }
                    ],
                    "next": "c"
                }
            },
            { "id": "b", "kind": "end" },
            { "id": "c", "kind": "end" }
        ]
    }));
    assert!(validate_workflow(&wf).is_ok());
}

#[test]
fn validates_rich_control_flow_node_targets() {
    let wf = workflow(serde_json::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": "route" } },
            {
                "id": "route",
                "kind": "switch",
                "parameters": {
                    "value": { "$value": "input#/mode" },
                    "cases": [
                        { "equals": "fanout", "target": "fanout" },
                        { "equals": "batch", "target": "batch" }
                    ],
                    "default": "emit"
                }
            },
            { "id": "fanout", "kind": "parallel", "parameters": { "branches": ["check_a", "check_b"] } },
            { "id": "check_a", "kind": "emit", "parameters": { "data": { "check": "a" } }, "transitions": { "next": "joined" } },
            { "id": "check_b", "kind": "emit", "parameters": { "data": { "check": "b" } }, "transitions": { "next": "joined" } },
            { "id": "joined", "kind": "join", "parameters": { "wait_for": ["check_a", "check_b"], "mode": "all" }, "transitions": { "next": "guarded" } },
            { "id": "guarded", "kind": "try", "parameters": { "body": "body", "catch": "catch", "finally": "finally" }, "transitions": { "next": "done" } },
            { "id": "body", "kind": "emit", "parameters": { "data": "body" }, "transitions": { "next": "guarded" } },
            { "id": "catch", "kind": "emit", "parameters": { "data": "catch" }, "transitions": { "next": "guarded" } },
            { "id": "finally", "kind": "emit", "parameters": { "data": "finally" }, "transitions": { "next": "guarded" } },
            { "id": "batch", "kind": "map", "parameters": { "items": [1, 2], "target": "map_item", "concurrency": 1 }, "transitions": { "next": "race" } },
            { "id": "map_item", "kind": "emit", "parameters": { "data": { "$value": "workflow.state#/map/item" } }, "transitions": { "next": "batch" } },
            { "id": "race", "kind": "race", "parameters": { "branches": ["fast", "slow"], "winner": "first_success" }, "transitions": { "next": "done" } },
            { "id": "fast", "kind": "emit", "parameters": { "data": "fast" }, "transitions": { "next": "race" } },
            { "id": "slow", "kind": "emit", "parameters": { "data": "slow" }, "transitions": { "next": "race" } },
            { "id": "emit", "kind": "emit", "parameters": { "event_type": "workflow.routed", "data": { "ok": true } }, "transitions": { "next": "done" } },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(validate_workflow(&wf).is_ok());
}

#[test]
fn rejects_missing_control_flow_target() {
    let wf = workflow(serde_json::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": "route" } },
            {
                "id": "route",
                "kind": "switch",
                "parameters": {
                    "value": "mode",
                    "cases": [{ "equals": "missing", "target": "missing" }]
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
            { "id": "start", "kind": "start", "transitions": { "next": "batch" } },
            { "id": "batch", "kind": "map", "parameters": { "items": [], "target": "item", "concurrency": 0 } },
            { "id": "item", "kind": "emit", "parameters": { "data": null }, "transitions": { "next": "batch" } },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidNodeParameters { .. })
    ));
}

#[test]
fn evaluates_switch_cases_and_default() {
    let node: WorkflowNode = serde_json::from_value(serde_json::json!({
        "id": "route",
        "kind": "switch",
        "parameters": {
            "value": { "$value": "input#/mode" },
            "cases": [
                { "equals": "fast", "target": "fast_path" },
                { "equals": "slow", "target": "slow_path" }
            ],
            "default": "fallback"
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
                "transitions": { "next": "step1" }
            },
            {
                "id": "step1",
                "kind": "task",
                "task_id": 1,
                "transitions": { "on_success": "step2", "on_failure": "failed" }
            },
            {
                "id": "step2",
                "kind": "condition",
                "transitions": {
                    "branches": [
                        { "when": { "value": { "$value": "steps.step1.output#/ok" }, "equals": true }, "target": "success" }
                    ],
                    "next": "failed"
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
            { "id": "build", "kind": "task", "task_id": 1, "transitions": {} }
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
    assert_eq!(build.transitions.next.as_deref(), Some("end"));
}
