//! the isolation guardrails on concurrent bodies (a map body may not be entered or read from
//! outside) and the acquire/release ordering a mutex section must keep.

use super::*;

#[test]
fn accepts_concurrent_map_with_single_action_body() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
            {
                "id": "map",
                "kind": "map",
                "parameters": { "items": [1, 2], "target": { "$node": "work" }, "concurrency": 2 },
                "transitions": { "on_success": { "$node": "done" } }
            },
            {
                "id": "work",
                "kind": "action",
                "action": { "provider": "console", "function": "run" },
                "transitions": { "on_success": { "$node": "map" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("single-action concurrent map body is isolatable");
}

#[test]
fn accepts_concurrent_map_with_multi_node_body() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
            {
                "id": "map",
                "kind": "map",
                "parameters": { "items": [1, 2], "target": { "$node": "a" }, "concurrency": 2 },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "a", "kind": "output", "transitions": { "on_success": { "$node": "b" } } },
            { "id": "b", "kind": "output", "transitions": { "on_success": { "$node": "map" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("multi-node isolatable concurrent map body validates");
}

#[test]
fn rejects_concurrent_map_when_body_entered_from_outside() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
            {
                "id": "map",
                "kind": "map",
                "parameters": { "items": [1, 2], "target": { "$node": "a" }, "concurrency": 2 },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "a", "kind": "output", "transitions": { "on_success": { "$node": "b" } } },
            { "id": "b", "kind": "output", "transitions": { "on_success": { "$node": "map" } } },
            { "id": "intruder", "kind": "output", "transitions": { "on_success": { "$node": "b" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MapConcurrencyBodyNotIsolatable { .. })
    ));
}

#[test]
fn rejects_concurrent_map_when_body_output_read_outside() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
            {
                "id": "map",
                "kind": "map",
                "parameters": { "items": [1, 2], "target": { "$node": "a" }, "concurrency": 2 },
                "transitions": { "on_success": { "$node": "combine" } }
            },
            { "id": "a", "kind": "output", "transitions": { "on_success": { "$node": "map" } } },
            {
                "id": "combine",
                "kind": "output",
                "parameters": { "data": { "$ref": { "node": "a", "output": ["data"] } } },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MapConcurrencyBodyNotIsolatable { .. })
    ));
}

#[test]
fn rejects_concurrent_map_with_terminal_node_in_body() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
            {
                "id": "map",
                "kind": "map",
                "parameters": { "items": [1, 2], "target": { "$node": "a" }, "concurrency": 2 },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "a", "kind": "output", "transitions": { "on_success": { "$node": "stop" } } },
            { "id": "stop", "kind": "end" },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MapConcurrencyBodyNotIsolatable { .. })
    ));
}

#[test]
fn serial_map_skips_isolation_guardrail() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
            {
                "id": "map",
                "kind": "map",
                "parameters": { "items": [1, 2], "target": { "$node": "a" } },
                "transitions": { "on_success": { "$node": "combine" } }
            },
            { "id": "a", "kind": "output", "transitions": { "on_success": { "$node": "map" } } },
            {
                "id": "combine",
                "kind": "output",
                "parameters": { "data": { "$ref": { "node": "a", "output": ["data"] } } },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    // without concurrency the body need not be isolatable; the guardrail does not apply.
    validate_workflow(&wf).expect("serial map is unaffected by the isolation guardrail");
}

#[test]
fn accepts_bracketed_mutex_section() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "acquire" } } },
            { "id": "acquire", "kind": "mutex", "parameters": { "name": "deploy" }, "transitions": { "next": { "$node": "work" } } },
            { "id": "work", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} }, "transitions": { "on_success": { "$node": "release" } } },
            { "id": "release", "kind": "mutex", "parameters": { "name": "deploy", "release": true }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("a release preceded by its acquire validates");
}

#[test]
fn rejects_mutex_release_before_acquire() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "release" } } },
            { "id": "release", "kind": "mutex", "parameters": { "name": "deploy", "release": true }, "transitions": { "next": { "$node": "acquire" } } },
            { "id": "acquire", "kind": "mutex", "parameters": { "name": "deploy" }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(matches!(
        validate_workflow(&wf),
        Err(WorkflowValidationError::MutexReleaseBeforeAcquire { .. })
    ));
}

#[test]
fn accepts_mutex_acquire_reached_in_a_loop() {
    // a self-reentrant acquire (re-reached by a loop back-edge) reinforces the hold at runtime; the
    // validator must not flag it, and there is no release before the acquire.
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "acquire" } } },
            { "id": "acquire", "kind": "mutex", "parameters": { "name": "deploy" }, "reentry": { "enabled": true, "max_visits": 5 }, "transitions": { "next": { "$node": "gate" } } },
            { "id": "gate", "kind": "condition", "reentry": { "enabled": true, "max_visits": 5 }, "transitions": { "branches": [ { "when": { "kind": "always" }, "target": { "$node": "acquire" } } ], "on_success": { "$node": "release" } } },
            { "id": "release", "kind": "mutex", "parameters": { "name": "deploy", "release": true }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow(&wf).expect("acquire in a loop with a later release validates");
}
