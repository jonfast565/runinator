//! control-flow node targets and the evaluators that pick a branch: transitions, reentry bounds,
//! switch cases, toggle state, and percentage buckets.

use super::*;
use runinator_models::workflows::WorkflowNodeRun;

fn completed_node_run(id: u128, node_id: &str) -> WorkflowNodeRun {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::from_u128(id),
        "workflow_run_id": Uuid::from_u128(1),
        "node_id": node_id,
        "status": "succeeded",
        "attempt": 1,
        "parameters": null,
        "output_json": null,
        "state": null,
        "transition_reason": null,
        "created_at": "2026-01-01T00:00:00Z",
        "started_at": "2026-01-01T00:00:00Z",
        "finished_at": "2026-01-01T00:00:01Z",
        "message": null,
    }))
    .expect("node run")
}

#[test]
fn a_race_visit_ignores_winners_from_previous_loop_laps() {
    let branches = vec!["fast".to_string(), "slow".to_string()];
    let current_race_run = Uuid::from_u128(100);
    let mut history = vec![completed_node_run(90, "fast")];

    assert_eq!(
        race_winner_since(
            &branches,
            BranchPolicy::FirstSuccess,
            &history,
            Some(current_race_run),
        ),
        None,
        "the prior lap's winner must not settle the current lap"
    );

    history.push(completed_node_run(110, "slow"));
    assert_eq!(
        race_winner_since(
            &branches,
            BranchPolicy::FirstSuccess,
            &history,
            Some(current_race_run),
        ),
        Some("slow".to_string())
    );
}

#[test]
fn an_all_race_counts_only_contenders_from_its_current_visit() {
    let branches = vec!["left".to_string(), "right".to_string()];
    let current_race_run = Uuid::from_u128(100);
    let mut history = vec![
        completed_node_run(80, "left"),
        completed_node_run(90, "right"),
        completed_node_run(110, "left"),
    ];

    assert_eq!(
        race_winner_since(
            &branches,
            BranchPolicy::All,
            &history,
            Some(current_race_run),
        ),
        None,
        "the current left contender cannot pair with the prior lap's right contender"
    );

    history.push(completed_node_run(120, "right"));
    assert_eq!(
        race_winner_since(
            &branches,
            BranchPolicy::All,
            &history,
            Some(current_race_run),
        ),
        Some("right".to_string())
    );
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
                    "branches": [{ "when": { "value": { "$ref": { "params": ["foo"] } }, "equals": "bar" }, "target": { "$node": "b" } }],
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
                    "value": { "$ref": { "params": ["mode"] } },
                    "cases": [
                        { "equals": "fanout", "target": { "$node": "fanout" } },
                        { "equals": "batch", "target": { "$node": "batch" } }
                    ],
                    "default": { "$node": "output" }
                }
            },
            { "id": "fanout", "kind": "parallel", "parameters": { "branches": [{ "$node": "check_a" }, { "$node": "check_b" }] } },
            { "id": "check_a", "kind": "output", "parameters": { "data": { "check": "a" } }, "transitions": { "next": { "$node": "joined" } } },
            { "id": "check_b", "kind": "output", "parameters": { "data": { "check": "b" } }, "transitions": { "next": { "$node": "joined" } } },
            { "id": "joined", "kind": "join", "parameters": { "wait_for": [{ "$node": "check_a" }, { "$node": "check_b" }], "mode": "all" }, "transitions": { "next": { "$node": "guarded" } } },
            { "id": "guarded", "kind": "try", "parameters": { "body": { "$node": "body" }, "catch": { "$node": "catch" }, "finally": { "$node": "finally" } }, "transitions": { "next": { "$node": "done" } } },
            { "id": "body", "kind": "output", "parameters": { "data": "body" }, "transitions": { "next": { "$node": "guarded" } } },
            { "id": "catch", "kind": "output", "parameters": { "data": "catch" }, "transitions": { "next": { "$node": "guarded" } } },
            { "id": "finally", "kind": "output", "parameters": { "data": "finally" }, "transitions": { "next": { "$node": "guarded" } } },
            { "id": "batch", "kind": "map", "parameters": { "items": [1, 2], "target": { "$node": "map_item" }, "concurrency": 1 }, "transitions": { "next": { "$node": "race" } } },
            { "id": "map_item", "kind": "output", "parameters": { "data": { "$ref": { "workflow": ["state", "map", "item"] } } }, "transitions": { "next": { "$node": "batch" } } },
            { "id": "race", "kind": "race", "parameters": { "branches": [{ "$node": "fast" }, { "$node": "slow" }], "winner": "first_success" }, "transitions": { "next": { "$node": "done" } } },
            { "id": "fast", "kind": "output", "parameters": { "data": "fast" }, "transitions": { "next": { "$node": "race" } } },
            { "id": "slow", "kind": "output", "parameters": { "data": "slow" }, "transitions": { "next": { "$node": "race" } } },
            { "id": "output", "kind": "output", "parameters": { "event_type": "workflow.routed", "data": { "ok": true } }, "transitions": { "next": { "$node": "done" } } },
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
            { "id": "item", "kind": "output", "parameters": { "data": null }, "transitions": { "next": { "$node": "batch" } } },
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
                    "items": { "$ref": { "params": ["tickets"] } }
                },
                "max_iterations": 50,
                "transitions": {
                    "next": { "$node": "process_ticket" },
                    "on_success": { "$node": "done" }
                }
            },
            {
                "id": "process_ticket",
                "kind": "output",
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
                "value": { "$ref": { "params": ["mode"] } },
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
fn evaluates_toggle_on_and_off() {
    let node: WorkflowNode = serde_json::from_value(
        runinator_models::json!({
            "id": "flag",
            "kind": "toggle",
            "parameters": {
                "value": { "$ref": { "config": ["flags", "new_checkout"] } },
                "on": { "$node": "new_checkout" },
                "off": { "$node": "old_checkout" }
            }
        })
        .into(),
    )
    .unwrap();
    let params = parse_toggle_parameters(&node).unwrap();

    assert_eq!(
        evaluate_toggle(
            &params,
            &runinator_models::json!({ "config": { "flags": { "new_checkout": true } } })
        )
        .unwrap(),
        "new_checkout"
    );
    assert_eq!(
        evaluate_toggle(
            &params,
            &runinator_models::json!({ "config": { "flags": { "new_checkout": false } } })
        )
        .unwrap(),
        "old_checkout"
    );
    // a missing/null value is falsy, so the toggle routes to `off`.
    assert_eq!(
        evaluate_toggle(&params, &runinator_models::json!({ "config": {} })).unwrap(),
        "old_checkout"
    );
}

#[test]
fn evaluates_percentage_buckets_stickily() {
    let node: WorkflowNode = serde_json::from_value(
        runinator_models::json!({
            "id": "rollout",
            "kind": "percentage",
            "parameters": {
                "key": { "$ref": { "input": ["user_id"] } },
                "buckets": [
                    { "weight": 30, "target": { "$node": "variant_a" } },
                    { "weight": 70, "target": { "$node": "variant_b" } }
                ],
                "default": { "$node": "control" }
            }
        })
        .into(),
    )
    .unwrap();
    let params = parse_percentage_parameters(&node).unwrap();

    let route = |user: &str| {
        evaluate_percentage(
            &params,
            &runinator_models::json!({ "input": { "user_id": user } }),
        )
        .unwrap()
    };

    // deterministic + sticky: the same key always lands in the same bucket.
    let first = route("user-42");
    assert_eq!(first, route("user-42"));
    assert!(matches!(
        first.as_deref(),
        Some("variant_a") | Some("variant_b")
    ));

    // a null key has nothing to hash, so it falls back to the default.
    assert_eq!(
        evaluate_percentage(&params, &runinator_models::json!({ "input": {} })).unwrap(),
        Some("control".into())
    );

    // the split roughly honors the configured weights across many keys.
    let variant_a = (0..1000)
        .filter(|id| route(&format!("user-{id}")).as_deref() == Some("variant_a"))
        .count();
    assert!(
        (150..450).contains(&variant_a),
        "expected ~30% in variant_a, got {variant_a}/1000"
    );
}
