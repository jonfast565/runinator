//! graph-shape validation: transitions that point nowhere, at a terminal, or into a control body,
//! and output references to nodes that produce none.

use super::*;

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
fn rejects_transition_targeting_start_node() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            {
                "id": "build",
                "kind": "action",
                "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} },
                "transitions": { "on_success": { "$node": "start" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidNodeReferenceType { .. })
    ));
}

#[test]
fn rejects_control_body_entry_targeting_terminal_node() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "batch" } } },
            {
                "id": "batch",
                "kind": "map",
                "parameters": {
                    "items": [1, 2],
                    "target": { "$node": "done" }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidNodeReferenceType { .. })
    ));
}

#[test]
fn rejects_node_output_ref_to_non_output_node() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "route" } } },
            {
                "id": "route",
                "kind": "condition",
                "transitions": { "next": { "$node": "consume" } }
            },
            {
                "id": "consume",
                "kind": "output",
                "parameters": {
                    "data": { "$ref": { "node": "route", "output": ["ok"] } }
                },
                "transitions": { "next": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidNodeReferenceType { .. })
    ));
}

#[test]
fn accepts_node_output_ref_to_approval_node() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "approve" } } },
            {
                "id": "approve",
                "kind": "approval",
                "transitions": { "on_success": { "$node": "record" } }
            },
            {
                "id": "record",
                "kind": "output",
                "parameters": {
                    "data": { "$ref": { "node": "approve", "output": ["approved"] } }
                },
                "transitions": { "next": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("approval nodes produce completion output");
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
fn rejects_string_value_reference_syntax() {
    let graph = WorkflowGraph::from_value(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": "build" } },
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} } },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(graph.is_err());

    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
            { "id": "build", "kind": "output", "parameters": { "data": { "$value": "params#/value" } }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::InvalidValueRef(_))
    ));
}
