use crate::context::*;
use crate::{
    api::WorkflowSchedulerApi, nodes::*, workflow::process_workflow_run,
    workflow::process_workflow_run_step,
};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use runinator_broker::Broker;
use runinator_broker::in_memory::InMemoryBroker;
use runinator_comm::{ActionCommand, ActionDispatchRecord, ControlKind};
use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    orchestration::ReadyNodeRecord,
    providers::{ActionMetadata, ProviderMetadata, ProviderRuntimeMetadata},
    workflows::{
        WorkflowDefinition, WorkflowNode, WorkflowNodeRun, WorkflowRun, WorkflowStatus,
        WorkflowTrigger,
    },
};
use std::sync::{Arc, Mutex};

#[test]
fn merges_parameters() {
    let defaults = json!({ "a": 1, "b": 2 });
    let overrides = json!({ "b": 3, "c": 4 });
    let merged = merge_parameters(&defaults, &overrides);
    assert_eq!(merged, json!({ "a": 1, "b": 3, "c": 4 }));
}

#[test]
fn builds_runtime_context() {
    let workflow_run = WorkflowRun {
        id: 10,
        workflow_id: 1,
        workflow_snapshot: None,
        status: WorkflowStatus::Running,
        active_node_id: Some("curr".into()),
        parameters: json!({ "name": "foo" }),
        state: json!({ "loop_index": 0 }),
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
        name: None,
    };
    let node_runs = vec![WorkflowNodeRun {
        id: 100,
        workflow_run_id: 10,
        node_id: "prev".into(),
        status: WorkflowStatus::Succeeded,
        attempt: 1,
        parameters: json!({}),
        output_json: Some(json!({ "result": "ok" })),
        state: json!({}),
        transition_reason: None,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
    }];

    let ctx = runtime_context(&workflow_run, &node_runs);
    assert_eq!(ctx["input"]["name"], "foo");
    assert_eq!(ctx["steps"]["prev"]["output"]["result"], "ok");
    assert_eq!(ctx["prev"]["result"], "ok");
    assert_eq!(ctx["workflow"]["run_id"], 10);
    assert_eq!(ctx["workflow"]["state"]["loop_index"], 0);
}

#[test]
fn initializes_next_execution_from_cron_schedule() {
    let now = Utc.with_ymd_and_hms(2026, 5, 9, 2, 30, 0).unwrap();
    let next = crate::db_extensions::next_execution_for_cron("0 0,9,12,15,18,21 * * *", now)
        .expect("cron schedule is valid");

    assert_eq!(next, Utc.with_ymd_and_hms(2026, 5, 9, 9, 0, 0).unwrap());
}

#[tokio::test]
async fn scheduler_uses_workflow_run_snapshot() {
    let mut run = workflow_run(json!({}), json!({}), "start");
    run.workflow_snapshot = Some(simple_workflow());
    let api = MockWorkflowApi {
        state: Mutex::new(MockWorkflowState {
            workflow: None,
            workflow_run: Some(run.clone()),
            ..Default::default()
        }),
    };
    let broker = InMemoryBroker::new();

    process_workflow_run(&broker, &api, run).await.unwrap();

    assert_eq!(api.last_run_update().active_node_id.as_deref(), Some("end"));
}

#[tokio::test]
async fn scheduler_marks_skipped_node_succeeded_without_dispatching() {
    let workflow = workflow_with_nodes(json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "build" } } },
        {
            "id": "build",
            "kind": "action",
            "skipped": true,
            "action": {
                "provider": "console",
                "function": "run",
                "timeout_seconds": 60,
                "configuration": {}
            },
            "transitions": { "next": { "$node": "end" } }
        },
        { "id": "end", "kind": "end" },
        { "id": "fail", "kind": "fail" }
    ]));
    let run = workflow_run(json!({}), json!({}), "build");
    let api = MockWorkflowApi::with_workflow_run(workflow, run.clone());
    api.state.lock().unwrap().providers = console_providers();
    let broker = InMemoryBroker::new();

    process_workflow_run(&broker, &api, run).await.unwrap();

    assert!(api.action_dispatches().is_empty());
    let skipped_update = api
        .node_updates()
        .into_iter()
        .find(|update| update.output_json["skipped"] == true)
        .expect("skipped node update");
    assert_eq!(skipped_update.status, WorkflowStatus::Succeeded);
    assert_eq!(api.last_run_update().active_node_id.as_deref(), Some("end"));
}

#[tokio::test]
async fn switch_routes_matching_default_and_unmatched_cases() {
    let run = workflow_run(json!({ "mode": "fast" }), json!({}), "route");
    let api = MockWorkflowApi::default();
    let route = node(json!({
        "id": "route",
        "kind": "switch",
        "parameters": {
            "value": { "$ref": { "input": ["mode"] } },
            "cases": [{ "equals": "fast", "target": { "$node": "fast_path" } }],
            "default": { "$node": "fallback" }
        }
    }));

    process_switch_node(&api, &run, &route, &[]).await.unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("fast_path")
    );

    let run = workflow_run(json!({ "mode": "slow" }), json!({}), "route");
    let api = MockWorkflowApi::default();
    process_switch_node(&api, &run, &route, &[]).await.unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("fallback")
    );

    let unmatched = node(json!({
        "id": "route",
        "kind": "switch",
        "parameters": {
            "value": { "$ref": { "input": ["mode"] } },
            "cases": [{ "equals": "fast", "target": { "$node": "fast_path" } }]
        }
    }));
    let api = MockWorkflowApi::default();
    process_switch_node(&api, &run, &unmatched, &[])
        .await
        .unwrap();
    assert_eq!(api.last_run_update().status, WorkflowStatus::Blocked);
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("route")
    );
}

#[tokio::test]
async fn emit_resolves_output_and_advances() {
    let run = workflow_run(json!({ "ticket": "RUN-7" }), json!({}), "emit");
    let api = MockWorkflowApi::default();
    let node = node(json!({
        "id": "emit",
        "kind": "emit",
        "parameters": {
            "event_type": "ticket.ready",
            "data": { "ticket": { "$ref": { "input": ["ticket"] } } }
        },
        "transitions": { "next": { "$node": "done" } }
    }));

    process_emit_node(&api, &run, &node, &[]).await.unwrap();

    let update = api.last_node_update();
    assert_eq!(update.status, WorkflowStatus::Succeeded);
    assert_eq!(update.output_json["event_type"], "ticket.ready");
    assert_eq!(update.output_json["data"]["ticket"], "RUN-7");
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
}

#[tokio::test]
async fn parallel_progresses_branches_into_join_all() {
    let parallel = node(json!({
        "id": "fanout",
        "kind": "parallel",
        "parameters": { "branches": [{ "$node": "a" }, { "$node": "b" }] }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "fanout");

    process_parallel_node(&api, &run, &parallel, None, &[])
        .await
        .unwrap();
    let update = api.last_run_update();
    assert_eq!(update.active_node_id.as_deref(), Some("a"));
    assert_eq!(update.state["parallel"]["remaining"], json!(["b"]));

    let join = node(json!({
        "id": "join",
        "kind": "join",
        "parameters": { "wait_for": [{ "$node": "a" }, { "$node": "b" }], "mode": "all" },
        "transitions": { "next": { "$node": "done" } }
    }));
    let run = workflow_run(json!({}), update.state.clone(), "join");
    let api = MockWorkflowApi::default();
    process_join_node(
        &api,
        &run,
        &join,
        None,
        &[node_run("a", WorkflowStatus::Succeeded)],
    )
    .await
    .unwrap();
    assert_eq!(api.last_run_update().active_node_id.as_deref(), Some("b"));

    let run = workflow_run(json!({}), api.last_run_update().state.clone(), "join");
    let api = MockWorkflowApi::default();
    process_join_node(
        &api,
        &run,
        &join,
        None,
        &[
            node_run("a", WorkflowStatus::Succeeded),
            node_run("b", WorkflowStatus::Succeeded),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
}

#[tokio::test]
async fn join_any_succeeds_with_one_completed_branch() {
    let join = node(json!({
        "id": "join",
        "kind": "join",
        "parameters": { "wait_for": [{ "$node": "a" }, { "$node": "b" }], "mode": "any" },
        "transitions": { "next": { "$node": "done" } }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "join");

    process_join_node(
        &api,
        &run,
        &join,
        None,
        &[node_run("a", WorkflowStatus::Succeeded)],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
}

#[tokio::test]
async fn try_routes_body_success_failure_and_finally() {
    let try_node = node(json!({
        "id": "guard",
        "kind": "try",
        "parameters": { "body": { "$node": "body" }, "catch": { "$node": "recover" }, "finally": { "$node": "cleanup" } },
        "transitions": { "next": { "$node": "done" } }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "guard");

    process_try_node(&api, &run, &try_node, None, &[])
        .await
        .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("body")
    );

    let latest = node_run_with_id(1, "guard", WorkflowStatus::Running, None, json!({}));
    let run = workflow_run(
        json!({}),
        json!({ "try": { "node_id": "guard", "phase": "body" } }),
        "guard",
    );
    let api = MockWorkflowApi::default();
    process_try_node(
        &api,
        &run,
        &try_node,
        Some(&latest),
        &[node_run("body", WorkflowStatus::Failed)],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("recover")
    );

    let run = workflow_run(
        json!({}),
        json!({ "try": { "node_id": "guard", "phase": "body" } }),
        "guard",
    );
    let api = MockWorkflowApi::default();
    process_try_node(
        &api,
        &run,
        &try_node,
        Some(&latest),
        &[node_run("body", WorkflowStatus::Succeeded)],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("cleanup")
    );
}

#[tokio::test]
async fn map_exposes_item_aggregates_output_and_propagates_failure() {
    let map = node(json!({
        "id": "batch",
        "kind": "map",
        "parameters": { "items": ["a", "b"], "target": { "$node": "item" }, "concurrency": 1 },
        "transitions": { "next": { "$node": "done" } }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "batch");

    process_map_node(&api, &run, &map, None, &[]).await.unwrap();
    let update = api.last_run_update();
    assert_eq!(update.active_node_id.as_deref(), Some("item"));
    assert_eq!(update.state["map"]["item"], "a");
    assert_eq!(update.state["map"]["index"], 0);

    let latest = node_run_with_id(
        1,
        "batch",
        WorkflowStatus::Running,
        None,
        update.state["map"].clone(),
    );
    let run = workflow_run(json!({}), update.state.clone(), "batch");
    let api = MockWorkflowApi::default();
    process_map_node(
        &api,
        &run,
        &map,
        Some(&latest),
        &[node_run_with_output(
            "item",
            WorkflowStatus::Succeeded,
            json!({ "ok": "a" }),
        )],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("item")
    );
    assert_eq!(api.last_run_update().state["map"]["item"], "b");
    let second_state = api.last_run_update().state;

    let latest = node_run_with_id(
        1,
        "batch",
        WorkflowStatus::Running,
        None,
        second_state["map"].clone(),
    );
    let run = workflow_run(json!({}), second_state, "batch");
    let api = MockWorkflowApi::default();
    process_map_node(
        &api,
        &run,
        &map,
        Some(&latest),
        &[
            node_run_with_id(
                1,
                "item",
                WorkflowStatus::Succeeded,
                Some(json!({ "ok": "a" })),
                json!({}),
            ),
            node_run_with_id(
                2,
                "item",
                WorkflowStatus::Succeeded,
                Some(json!({ "ok": "b" })),
                json!({}),
            ),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
    assert_eq!(api.last_node_update().output_json["count"], 2);

    let failed_run = workflow_run(json!({}), update.state, "batch");
    let api = MockWorkflowApi::default();
    process_map_node(
        &api,
        &failed_run,
        &map,
        Some(&latest),
        &[node_run("item", WorkflowStatus::Failed)],
    )
    .await
    .unwrap();
    assert_eq!(api.last_run_update().status, WorkflowStatus::Failed);
}

#[tokio::test]
async fn race_records_first_success_and_starts_remaining_branches_sequentially() {
    let race = node(json!({
        "id": "race",
        "kind": "race",
        "parameters": { "branches": [{ "$node": "fast" }, { "$node": "slow" }], "winner": "first_success" },
        "transitions": { "next": { "$node": "done" } }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "race");

    process_race_node(&api, &run, &race, None, &[])
        .await
        .unwrap();
    let update = api.last_run_update();
    assert_eq!(update.active_node_id.as_deref(), Some("fast"));
    assert_eq!(update.state["race"]["remaining"], json!(["slow"]));

    let latest = node_run_with_id(1, "race", WorkflowStatus::Running, None, json!({}));
    let run = workflow_run(json!({}), update.state, "race");
    let api = MockWorkflowApi::default();
    process_race_node(
        &api,
        &run,
        &race,
        Some(&latest),
        &[node_run("fast", WorkflowStatus::Succeeded)],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
    assert_eq!(api.last_node_update().output_json["winner"], "fast");
}

#[test]
fn reentry_exhaustion_routes_after_max_visits() {
    let node = node(json!({
        "id": "implement",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
        "reentry": {
            "enabled": true,
            "max_visits": 2,
            "on_exhausted": { "$node": "deferred" }
        }
    }));
    let runs = vec![
        node_run_with_id(1, "implement", WorkflowStatus::Succeeded, None, json!({})),
        node_run_with_id(2, "implement", WorkflowStatus::Failed, None, json!({})),
    ];

    assert_eq!(
        crate::workflow::reentry_exhaustion(&node, Some(&runs[1]), &runs),
        Some(crate::workflow::ReentryExhaustion::Route("deferred".into()))
    );
}

#[test]
fn reentry_exhaustion_ignores_active_latest_visit() {
    let node = node(json!({
        "id": "implement",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
        "reentry": { "enabled": true, "max_visits": 1 }
    }));
    let running = node_run_with_id(1, "implement", WorkflowStatus::Running, None, json!({}));

    assert_eq!(
        crate::workflow::reentry_exhaustion(&node, Some(&running), std::slice::from_ref(&running)),
        None
    );
}

#[test]
fn task_idempotency_key_includes_node_run_id() {
    let first = workflow_task_idempotency_key(10, "implement", 1, 1);
    let second = workflow_task_idempotency_key(10, "implement", 2, 1);

    assert_ne!(first, second);
    assert_eq!(first, "10:implement:1:1");
}

#[tokio::test]
async fn debug_workflow_pauses_before_first_node() {
    let workflow = simple_workflow();
    let run = workflow_run(
        json!({ "name": "debug" }),
        json!({ "debug": { "enabled": true, "paused": false, "step_requested": false } }),
        "start",
    );
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    let update = api.last_run_update();
    assert_eq!(update.status, WorkflowStatus::DebugPaused);
    assert_eq!(update.active_node_id.as_deref(), Some("start"));
    assert_eq!(update.state["debug"]["paused"], true);
    assert_eq!(update.state["debug"]["current_node_id"], "start");
    assert_eq!(api.node_update_count(), 0);
}

#[tokio::test]
async fn queued_start_to_action_is_dispatched_in_one_pass() {
    let workflow = workflow_with_nodes(json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
        action_node("run"),
        { "id": "end", "kind": "end" }
    ]));
    let mut run = workflow_run(json!({}), json!({}), "start");
    run.status = WorkflowStatus::Queued;
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    api.state.lock().unwrap().providers = console_providers();
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    let update = api.last_run_update();
    assert_eq!(update.status, WorkflowStatus::Running);
    assert_eq!(update.active_node_id.as_deref(), Some("run"));
    assert_eq!(api.last_node_update().status, WorkflowStatus::Running);
    assert_eq!(api.action_dispatches().len(), 1);
}

#[tokio::test]
async fn synchronous_nodes_advance_to_action_in_one_pass() {
    let workflow = workflow_with_nodes(json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "condition" } } },
        {
            "id": "condition",
            "kind": "condition",
            "condition": { "value": true, "equals": true },
            "transitions": { "on_success": { "$node": "emit" } }
        },
        {
            "id": "emit",
            "kind": "emit",
            "parameters": { "event_type": "test.ready", "data": { "ok": true } },
            "transitions": { "next": { "$node": "run" } }
        },
        action_node("run"),
        { "id": "end", "kind": "end" }
    ]));
    let mut run = workflow_run(json!({}), json!({}), "start");
    run.status = WorkflowStatus::Queued;
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    api.state.lock().unwrap().providers = console_providers();
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    assert_eq!(api.last_run_update().active_node_id.as_deref(), Some("run"));
    assert_eq!(api.last_node_update().status, WorkflowStatus::Running);
    assert_eq!(api.state.lock().unwrap().node_runs.len(), 4);
}

#[tokio::test]
async fn fail_node_marks_workflow_failed() {
    let workflow = simple_workflow();
    let run = workflow_run(json!({}), json!({}), "fail");
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    assert_eq!(api.last_run_update().status, WorkflowStatus::Failed);
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("fail")
    );
    assert_eq!(api.last_node_update().status, WorkflowStatus::Succeeded);
}

#[tokio::test]
async fn debug_step_executes_one_node_and_clears_step_request() {
    let workflow = simple_workflow();
    let run = workflow_run(
        json!({}),
        json!({ "debug": { "enabled": true, "paused": true, "step_requested": true } }),
        "start",
    );
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    let updates = api.run_updates();
    assert_eq!(updates[0].status, WorkflowStatus::Running);
    assert_eq!(updates[0].state["debug"]["paused"], false);
    assert_eq!(updates[0].state["debug"]["step_requested"], false);
    assert_eq!(api.last_run_update().active_node_id.as_deref(), Some("end"));
    assert_eq!(api.last_node_update().status, WorkflowStatus::Succeeded);
}

#[tokio::test]
async fn debug_workflow_pauses_before_next_node_after_step() {
    let workflow = simple_workflow();
    let run = workflow_run(
        json!({}),
        json!({ "debug": { "enabled": true, "paused": false, "step_requested": false } }),
        "end",
    );
    let api = MockWorkflowApi::with_workflow_run_and_nodes(
        workflow,
        run,
        vec![node_run_with_output(
            "start",
            WorkflowStatus::Succeeded,
            json!({ "ok": true }),
        )],
    );
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    let update = api.last_run_update();
    assert_eq!(update.status, WorkflowStatus::DebugPaused);
    assert_eq!(update.active_node_id.as_deref(), Some("end"));
    assert_eq!(update.state["debug"]["last_output_json"]["ok"], true);
    assert_eq!(api.node_update_count(), 0);
}

#[tokio::test]
async fn breakpoints_mode_does_not_pause_on_non_breakpoint_nodes() {
    let workflow = simple_workflow();
    let run = workflow_run(
        json!({}),
        json!({
            "debug": {
                "enabled": true,
                "paused": false,
                "step_requested": false,
                "mode": "breakpoints",
                "breakpoints": ["end"]
            }
        }),
        "start",
    );
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    // should advance past start and pause at end (which is in breakpoints).
    let update = api.last_run_update();
    assert_eq!(update.status, WorkflowStatus::DebugPaused);
    assert_eq!(update.active_node_id.as_deref(), Some("end"));
}

#[tokio::test]
async fn breakpoints_mode_runs_through_when_no_breakpoints_match() {
    let workflow = simple_workflow();
    let run = workflow_run(
        json!({}),
        json!({
            "debug": {
                "enabled": true,
                "paused": false,
                "step_requested": false,
                "mode": "breakpoints",
                "breakpoints": []
            }
        }),
        "start",
    );
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    // with no matching breakpoints the run should advance until it stops naturally.
    let updates = api.run_updates();
    assert!(
        !updates
            .iter()
            .any(|u| u.status == WorkflowStatus::DebugPaused),
        "expected no DebugPaused transitions, got {:?}",
        updates.iter().map(|u| u.status).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn one_shot_breakpoint_pauses_then_clears() {
    let workflow = simple_workflow();
    let run = workflow_run(
        json!({}),
        json!({
            "debug": {
                "enabled": true,
                "paused": false,
                "step_requested": false,
                "mode": "breakpoints",
                "breakpoints": [],
                "one_shot_breakpoint": "end"
            }
        }),
        "start",
    );
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    let update = api.last_run_update();
    assert_eq!(update.status, WorkflowStatus::DebugPaused);
    assert_eq!(update.active_node_id.as_deref(), Some("end"));
    // the one_shot_breakpoint should be cleared after consumption.
    assert!(update.state["debug"]["one_shot_breakpoint"].is_null());
}

#[tokio::test]
async fn canceled_run_does_not_advance() {
    let workflow = simple_workflow();
    let mut run = workflow_run(json!({}), json!({}), "start");
    run.status = WorkflowStatus::Canceled;
    let api = MockWorkflowApi::with_workflow_run(workflow, run);
    let broker = InMemoryBroker::new();
    let run = api.state.lock().unwrap().workflow_run.clone().unwrap();

    process_workflow_run(&broker, &api, run).await.unwrap();

    // no updates should be produced for a terminal canceled run.
    assert!(api.run_updates().is_empty());
    assert_eq!(api.node_update_count(), 0);
}

#[tokio::test]
async fn fire_and_forget_subflow_creates_named_child_and_advances() {
    let run = workflow_run(
        json!({ "ticket": { "key": "ITP-123" } }),
        json!({}),
        "spawn",
    );
    let child = workflow_definition_with_id(42, "Ticket Work");
    let api = MockWorkflowApi {
        state: Mutex::new(MockWorkflowState {
            workflow_run: Some(run.clone()),
            workflows: vec![child],
            ..Default::default()
        }),
    };
    let node = node(json!({
        "id": "spawn",
        "kind": "subflow",
        "subflow": {
            "workflow_name": "Ticket Work",
            "type": "fire_and_forget",
            "run_name": {
                "$concat": ["Ticket Work: ", { "$ref": { "input": ["ticket", "key"] } }]
            },
            "reuse_open_run": true
        },
        "parameters": {
            "ticket": { "$ref": { "input": ["ticket"] } },
            "parent_workflow_run_id": { "$ref": { "workflow": ["run_id"] } }
        },
        "transitions": { "on_success": { "$node": "done" } }
    }));

    process_subflow_node(&api, &run, &node, None, &[])
        .await
        .unwrap();

    let created = api.created_workflow_runs();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].workflow_id, 42);
    assert_eq!(created[0].name.as_deref(), Some("Ticket Work: ITP-123"));
    assert_eq!(created[0].parameters["ticket"]["key"], "ITP-123");
    assert_eq!(created[0].parameters["parent_workflow_run_id"], 10);
    assert_eq!(api.last_node_update().status, WorkflowStatus::Succeeded);
    assert_eq!(
        api.last_node_update().output_json["subflow_run_id"],
        created[0].id
    );
    assert_eq!(api.last_node_update().output_json["reused"], false);
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
}

#[tokio::test]
async fn fire_and_forget_subflow_reuses_open_named_child() {
    let run = workflow_run(
        json!({ "ticket": { "key": "ITP-123" } }),
        json!({}),
        "spawn",
    );
    let existing = WorkflowRun {
        id: 99,
        workflow_id: 42,
        workflow_snapshot: Some(workflow_definition_with_id(42, "Ticket Work")),
        status: WorkflowStatus::Running,
        active_node_id: Some("implement".into()),
        parameters: json!({ "ticket": { "key": "ITP-123" } }),
        state: json!({}),
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
        name: Some("Ticket Work: ITP-123".into()),
    };
    let api = MockWorkflowApi {
        state: Mutex::new(MockWorkflowState {
            workflow_run: Some(run.clone()),
            workflows: vec![workflow_definition_with_id(42, "Ticket Work")],
            workflow_runs: vec![existing],
            ..Default::default()
        }),
    };
    let node = node(json!({
        "id": "spawn",
        "kind": "subflow",
        "subflow": {
            "workflow_name": "Ticket Work",
            "type": "fire_and_forget",
            "run_name": "Ticket Work: ITP-123",
            "reuse_open_run": true
        },
        "transitions": { "on_success": { "$node": "done" } }
    }));

    process_subflow_node(&api, &run, &node, None, &[])
        .await
        .unwrap();

    assert!(api.created_workflow_runs().is_empty());
    assert_eq!(api.last_node_update().output_json["subflow_run_id"], 99);
    assert_eq!(api.last_node_update().output_json["reused"], true);
}

#[tokio::test]
async fn fire_and_forget_subflow_ignores_terminal_named_child() {
    let run = workflow_run(json!({}), json!({}), "spawn");
    let terminal = WorkflowRun {
        id: 99,
        workflow_id: 42,
        workflow_snapshot: Some(workflow_definition_with_id(42, "Ticket Work")),
        status: WorkflowStatus::Succeeded,
        active_node_id: Some("done".into()),
        parameters: json!({}),
        state: json!({}),
        created_at: Utc::now(),
        started_at: None,
        finished_at: Some(Utc::now()),
        message: None,
        name: Some("Ticket Work: ITP-123".into()),
    };
    let api = MockWorkflowApi {
        state: Mutex::new(MockWorkflowState {
            workflow_run: Some(run.clone()),
            workflows: vec![workflow_definition_with_id(42, "Ticket Work")],
            workflow_runs: vec![terminal],
            ..Default::default()
        }),
    };
    let node = node(json!({
        "id": "spawn",
        "kind": "subflow",
        "subflow": {
            "workflow_name": "Ticket Work",
            "type": "fire_and_forget",
            "run_name": "Ticket Work: ITP-123",
            "reuse_open_run": true
        },
        "transitions": { "on_success": { "$node": "done" } }
    }));

    process_subflow_node(&api, &run, &node, None, &[])
        .await
        .unwrap();

    let created = api.created_workflow_runs();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name.as_deref(), Some("Ticket Work: ITP-123"));
}

#[tokio::test]
async fn legacy_subflow_defaults_to_waiting_child() {
    let run = workflow_run(json!({ "value": 7 }), json!({}), "spawn");
    let api = MockWorkflowApi::default();
    let node = node(json!({
        "id": "spawn",
        "kind": "subflow",
        "subflow_id": 42,
        "parameters": { "value": { "$ref": { "input": ["value"] } } },
        "transitions": { "on_success": { "$node": "done" } }
    }));

    process_subflow_node(&api, &run, &node, None, &[])
        .await
        .unwrap();

    let created = api.created_workflow_runs();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].workflow_id, 42);
    assert_eq!(created[0].parameters["value"], 7);
    assert_eq!(api.last_node_update().status, WorkflowStatus::Waiting);
    assert_eq!(api.last_run_update().status, WorkflowStatus::Waiting);
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("spawn")
    );
}

#[tokio::test]
async fn subflow_setup_failure_marks_node_failed_without_retrying() {
    let run = workflow_run(json!({}), json!({}), "spawn");
    let api = MockWorkflowApi::with_workflow_run(simple_workflow(), run.clone());
    let node = node(json!({
        "id": "spawn",
        "kind": "subflow",
        "retry": { "max_attempts": 3 },
        "subflow": {
            "workflow_name": "Missing Workflow"
        },
        "transitions": { "on_success": { "$node": "done" } }
    }));

    process_subflow_node(&api, &run, &node, None, &[])
        .await
        .unwrap();

    assert!(api.created_workflow_runs().is_empty());
    assert_eq!(api.last_node_update().status, WorkflowStatus::Failed);
    assert_eq!(api.last_run_update().status, WorkflowStatus::Failed);
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("spawn")
    );
}

#[tokio::test]
async fn waiting_subflow_times_out_after_timeout_seconds() {
    let run = workflow_run(json!({}), json!({}), "spawn");
    let child = WorkflowRun {
        id: 99,
        workflow_id: 42,
        workflow_snapshot: Some(workflow_definition_with_id(42, "Ticket Work")),
        status: WorkflowStatus::Waiting,
        active_node_id: Some("approval".into()),
        parameters: json!({ "ticket": "ITP-123" }),
        state: json!({ "phase": "approval" }),
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
        name: Some("Ticket Work: ITP-123".into()),
    };
    let api = MockWorkflowApi {
        state: Mutex::new(MockWorkflowState {
            workflow_run: Some(run.clone()),
            workflow_runs: vec![child],
            ..Default::default()
        }),
    };
    let node = node(json!({
        "id": "spawn",
        "kind": "subflow",
        "subflow_id": 42,
        "timeout_seconds": 1,
        "transitions": { "on_success": { "$node": "done" } }
    }));
    let mut node_run = node_run_with_id(
        7,
        "spawn",
        WorkflowStatus::Waiting,
        None,
        json!({
            "subflow_run_id": 99,
            "subflow_workflow_id": 42,
            "run_name": "Ticket Work: ITP-123",
            "reused": false
        }),
    );
    node_run.created_at = Utc::now() - chrono::Duration::seconds(2);

    process_subflow_node(&api, &run, &node, Some(&node_run), &[node_run.clone()])
        .await
        .unwrap();

    assert_eq!(api.last_node_update().status, WorkflowStatus::TimedOut);
    assert_eq!(
        api.last_node_update().output_json,
        json!({
            "subflow_run_id": 99,
            "status": "waiting"
        })
    );
    assert_eq!(api.last_run_update().status, WorkflowStatus::TimedOut);
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("spawn")
    );
}

#[derive(Debug, Clone)]
struct WorkflowRunUpdate {
    status: WorkflowStatus,
    active_node_id: Option<String>,
    state: Value,
}

#[derive(Debug, Clone)]
struct WorkflowNodeRunUpdate {
    status: WorkflowStatus,
    output_json: Value,
}

#[derive(Default)]
struct MockWorkflowApi {
    state: Mutex<MockWorkflowState>,
}

#[derive(Default)]
struct MockWorkflowState {
    next_workflow_run_id: i64,
    next_node_run_id: i64,
    workflow: Option<WorkflowDefinition>,
    workflows: Vec<WorkflowDefinition>,
    workflow_run: Option<WorkflowRun>,
    workflow_runs: Vec<WorkflowRun>,
    created_workflow_runs: Vec<WorkflowRun>,
    node_runs: Vec<WorkflowNodeRun>,
    workflow_updates: Vec<WorkflowRunUpdate>,
    node_updates: Vec<WorkflowNodeRunUpdate>,
    action_dispatches: Vec<ActionDispatchRecord>,
    providers: Vec<ProviderMetadata>,
}

impl MockWorkflowApi {
    fn with_workflow_run(workflow: WorkflowDefinition, run: WorkflowRun) -> Self {
        Self {
            state: Mutex::new(MockWorkflowState {
                workflow: Some(workflow),
                workflow_run: Some(run),
                ..Default::default()
            }),
        }
    }

    fn with_workflow_run_and_nodes(
        workflow: WorkflowDefinition,
        run: WorkflowRun,
        node_runs: Vec<WorkflowNodeRun>,
    ) -> Self {
        Self {
            state: Mutex::new(MockWorkflowState {
                workflow: Some(workflow),
                workflow_run: Some(run),
                node_runs,
                ..Default::default()
            }),
        }
    }

    fn last_run_update(&self) -> WorkflowRunUpdate {
        self.state
            .lock()
            .unwrap()
            .workflow_updates
            .last()
            .cloned()
            .expect("workflow run update")
    }

    fn last_node_update(&self) -> WorkflowNodeRunUpdate {
        self.state
            .lock()
            .unwrap()
            .node_updates
            .last()
            .cloned()
            .expect("workflow node run update")
    }

    fn node_updates(&self) -> Vec<WorkflowNodeRunUpdate> {
        self.state.lock().unwrap().node_updates.clone()
    }

    fn run_updates(&self) -> Vec<WorkflowRunUpdate> {
        self.state.lock().unwrap().workflow_updates.clone()
    }

    fn node_update_count(&self) -> usize {
        self.state.lock().unwrap().node_updates.len()
    }

    fn created_workflow_runs(&self) -> Vec<WorkflowRun> {
        self.state.lock().unwrap().created_workflow_runs.clone()
    }

    fn action_dispatches(&self) -> Vec<ActionDispatchRecord> {
        self.state.lock().unwrap().action_dispatches.clone()
    }
}

#[async_trait]
impl WorkflowSchedulerApi for MockWorkflowApi {
    async fn fetch_workflow(&self, _workflow_id: i64) -> Result<WorkflowDefinition, SendableError> {
        self.state
            .lock()
            .unwrap()
            .workflow
            .clone()
            .ok_or_else(|| test_error("unexpected workflow fetch"))
    }

    async fn fetch_workflow_by_name(
        &self,
        name: &str,
    ) -> Result<WorkflowDefinition, SendableError> {
        let state = self.state.lock().unwrap();
        state
            .workflows
            .iter()
            .find(|workflow| workflow.name == name)
            .cloned()
            .or_else(|| {
                state
                    .workflow
                    .clone()
                    .filter(|workflow| workflow.name == name)
            })
            .ok_or_else(|| test_error("unexpected workflow name fetch"))
    }

    async fn fetch_providers(&self) -> Result<Vec<ProviderMetadata>, SendableError> {
        Ok(self.state.lock().unwrap().providers.clone())
    }

    async fn create_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
    ) -> Result<WorkflowRun, SendableError> {
        self.create_named_workflow_run(workflow_id, parameters, String::new())
            .await
            .map(|mut run| {
                run.name = None;
                run
            })
    }

    async fn create_named_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
        name: String,
    ) -> Result<WorkflowRun, SendableError> {
        let mut state = self.state.lock().unwrap();
        state.next_workflow_run_id += 1;
        let workflow_snapshot = state
            .workflows
            .iter()
            .find(|workflow| workflow.id == Some(workflow_id))
            .cloned()
            .or_else(|| state.workflow.clone());
        let run = WorkflowRun {
            id: state.next_workflow_run_id,
            workflow_id,
            workflow_snapshot,
            status: WorkflowStatus::Queued,
            active_node_id: None,
            parameters,
            state: json!({ "control": { "pause_requested": false } }),
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            message: None,
            name: Some(name).filter(|name| !name.is_empty()),
        };
        state.created_workflow_runs.push(run.clone());
        state.workflow_runs.push(run.clone());
        Ok(run)
    }

    async fn fetch_due_workflow_triggers(&self) -> Result<Vec<WorkflowTrigger>, SendableError> {
        Ok(Vec::new())
    }

    async fn claim_due_workflow_trigger_firings(
        &self,
        _scheduler_id: &str,
        _limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(Vec::new())
    }

    async fn update_workflow_trigger_next_execution(
        &self,
        _trigger_id: i64,
        _next_execution: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), SendableError> {
        Ok(())
    }

    async fn fetch_workflow_runs_by_status(
        &self,
        _status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(Vec::new())
    }

    async fn claim_workflow_runs_for_scheduler(
        &self,
        _scheduler_id: &str,
        _statuses: &[WorkflowStatus],
        _lease_until: chrono::DateTime<Utc>,
        _limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(Vec::new())
    }

    async fn renew_workflow_run_claim(
        &self,
        _workflow_run_id: i64,
        _scheduler_id: &str,
        _lease_until: chrono::DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        Ok(true)
    }

    async fn release_workflow_run_claim(
        &self,
        _workflow_run_id: i64,
        _scheduler_id: &str,
    ) -> Result<(), SendableError> {
        Ok(())
    }

    async fn fetch_workflow_runs_by_name(
        &self,
        name: &str,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .workflow_runs
            .iter()
            .filter(|run| run.name.as_deref() == Some(name))
            .filter(|run| !open_only || !run.status.is_terminal())
            .cloned()
            .collect())
    }

    async fn update_workflow_run(
        &self,
        _workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        _message: Option<String>,
    ) -> Result<(), SendableError> {
        let mut state_guard = self.state.lock().unwrap();
        if let Some(run) = state_guard.workflow_run.as_mut() {
            run.status = status;
            run.active_node_id = active_node_id.clone();
            if let Some(next_state) = state.clone() {
                run.state = next_state;
            }
        }
        state_guard.workflow_updates.push(WorkflowRunUpdate {
            status,
            active_node_id,
            state: state.unwrap_or_else(|| json!({})),
        });
        Ok(())
    }

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<(WorkflowRun, Vec<WorkflowNodeRun>), SendableError> {
        let state = self.state.lock().unwrap();
        if let Some(run) = state
            .workflow_run
            .clone()
            .filter(|run| run.id == workflow_run_id)
        {
            return Ok((run, state.node_runs.clone()));
        }
        if let Some(run) = state
            .workflow_runs
            .iter()
            .find(|run| run.id == workflow_run_id)
            .cloned()
        {
            return Ok((run, Vec::new()));
        }
        Err(test_error("unexpected workflow run fetch"))
    }

    async fn set_workflow_run_name(
        &self,
        _workflow_run_id: i64,
        name: Option<String>,
    ) -> Result<(), SendableError> {
        let mut state = self.state.lock().unwrap();
        if let Some(run) = state.workflow_run.as_mut() {
            run.name = name;
        }
        Ok(())
    }

    async fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: &str,
        parameters: Value,
    ) -> Result<WorkflowNodeRun, SendableError> {
        let mut state = self.state.lock().unwrap();
        state.next_node_run_id += 1;
        let node_run = WorkflowNodeRun {
            id: state.next_node_run_id,
            workflow_run_id,
            node_id: node_id.into(),
            status: WorkflowStatus::Queued,
            attempt: 0,
            parameters,
            output_json: None,
            state: json!({}),
            transition_reason: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            message: None,
        };
        state.node_runs.push(node_run.clone());
        Ok(node_run)
    }

    async fn update_workflow_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let mut state_guard = self.state.lock().unwrap();
        if let Some(node_run) = state_guard
            .node_runs
            .iter_mut()
            .find(|node_run| node_run.id == node_run_id)
        {
            node_run.status = status;
            if let Some(attempt) = attempt {
                node_run.attempt = attempt;
            }
            if let Some(parameters) = parameters {
                node_run.parameters = parameters;
            }
            if let Some(output_json) = output_json.clone() {
                node_run.output_json = Some(output_json);
            }
            if let Some(state) = state {
                node_run.state = state;
            }
            if transition_reason.is_some() {
                node_run.transition_reason = transition_reason;
            }
            if message.is_some() {
                node_run.message = message;
            }
        }
        state_guard.node_updates.push(WorkflowNodeRunUpdate {
            status,
            output_json: output_json.unwrap_or(Value::Null),
        });
        Ok(())
    }

    async fn create_automation_record(
        &self,
        _path: &str,
        _record: Value,
    ) -> Result<Value, SendableError> {
        Err(test_error("unexpected automation record creation"))
    }

    async fn fetch_idempotency_key(
        &self,
        _scope: &str,
        _key: &str,
    ) -> Result<Option<Value>, SendableError> {
        Ok(None)
    }

    async fn put_idempotency_key(
        &self,
        _scope: &str,
        _key: &str,
        result: Value,
    ) -> Result<Value, SendableError> {
        Ok(result)
    }

    async fn enqueue_action_dispatch(
        &self,
        dedupe_key: &str,
        command: &ActionCommand,
    ) -> Result<ActionDispatchRecord, SendableError> {
        let mut state = self.state.lock().unwrap();
        if let Some(existing) = state
            .action_dispatches
            .iter()
            .find(|dispatch| dispatch.dedupe_key == dedupe_key)
            .cloned()
        {
            return Ok(existing);
        }
        let record = ActionDispatchRecord {
            id: state.action_dispatches.len() as i64 + 1,
            dedupe_key: dedupe_key.into(),
            command: command.clone(),
            attempts: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            published_at: None,
            last_error: None,
            claimed_by: None,
            claimed_until: None,
        };
        state.action_dispatches.push(record.clone());
        Ok(record)
    }

    async fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>, SendableError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .action_dispatches
            .iter()
            .filter(|dispatch| dispatch.published_at.is_none())
            .take(limit.max(1) as usize)
            .cloned()
            .collect())
    }

    async fn claim_ready_nodes(
        &self,
        _scheduler_id: &str,
        _lease_until: chrono::DateTime<Utc>,
        _limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>, SendableError> {
        Ok(Vec::new())
    }

    async fn process_ready_node(
        &self,
        _ready_node_id: i64,
        _scheduler_id: &str,
        _workflow_run_id: Option<i64>,
        _node_id: Option<String>,
        _next_ready_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), SendableError> {
        Ok(())
    }

    async fn claim_pending_action_dispatches(
        &self,
        _scheduler_id: &str,
        _lease_until: chrono::DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>, SendableError> {
        self.fetch_pending_action_dispatches(limit).await
    }

    async fn mark_action_dispatch_published(&self, dispatch_id: i64) -> Result<(), SendableError> {
        if let Some(dispatch) = self
            .state
            .lock()
            .unwrap()
            .action_dispatches
            .iter_mut()
            .find(|dispatch| dispatch.id == dispatch_id)
        {
            dispatch.published_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn mark_action_dispatch_failed(
        &self,
        dispatch_id: i64,
        error: &str,
    ) -> Result<(), SendableError> {
        if let Some(dispatch) = self
            .state
            .lock()
            .unwrap()
            .action_dispatches
            .iter_mut()
            .find(|dispatch| dispatch.id == dispatch_id)
        {
            dispatch.attempts += 1;
            dispatch.last_error = Some(error.into());
        }
        Ok(())
    }
}

fn node(value: Value) -> WorkflowNode {
    serde_json::from_value(value.into()).unwrap()
}

fn workflow_run(parameters: Value, state: Value, active: &str) -> WorkflowRun {
    WorkflowRun {
        id: 10,
        workflow_id: 1,
        workflow_snapshot: None,
        status: WorkflowStatus::Running,
        active_node_id: Some(active.into()),
        parameters,
        state,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
        name: None,
    }
}

fn simple_workflow() -> WorkflowDefinition {
    workflow_with_nodes(json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
        { "id": "end", "kind": "end" },
        { "id": "fail", "kind": "fail" }
    ]))
}

fn workflow_with_nodes(nodes: Value) -> WorkflowDefinition {
    WorkflowDefinition {
        id: Some(1),
        name: "debug".into(),
        version: 1,
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: runinator_models::workflows::WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": nodes
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    }
}

fn workflow_definition_with_id(id: i64, name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: Some(id),
        name: name.into(),
        version: 1,
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: runinator_models::workflows::WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
                { "id": "done", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    }
}

fn console_providers() -> Vec<ProviderMetadata> {
    vec![ProviderMetadata {
        name: "console".into(),
        actions: vec![ActionMetadata::new("run", "run")],
        metadata: ProviderRuntimeMetadata::default(),
    }]
}

fn action_node(id: &str) -> Value {
    json!({
        "id": id,
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
        "transitions": { "next": { "$node": "end" } }
    })
}

fn node_run(node_id: &str, status: WorkflowStatus) -> WorkflowNodeRun {
    node_run_with_id(1, node_id, status, None, json!({}))
}

fn node_run_with_output(node_id: &str, status: WorkflowStatus, output: Value) -> WorkflowNodeRun {
    node_run_with_id(1, node_id, status, Some(output), json!({}))
}

fn node_run_with_id(
    id: i64,
    node_id: &str,
    status: WorkflowStatus,
    output_json: Option<Value>,
    state: Value,
) -> WorkflowNodeRun {
    WorkflowNodeRun {
        id,
        workflow_run_id: 10,
        node_id: node_id.into(),
        status,
        attempt: 1,
        parameters: json!({}),
        output_json,
        state,
        transition_reason: None,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
    }
}

#[tokio::test]
async fn wait_node_times_out() {
    let node_id = "wait_node";
    let workflow = workflow_with_nodes(runinator_models::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": node_id } } },
        {
            "id": node_id,
            "kind": "wait",
            "wait": { "seconds": 100 },
            "timeout_seconds": 1
        },
        { "id": "end", "kind": "end" }
    ]));
    let mut run = workflow_run(
        runinator_models::json!({}),
        runinator_models::json!({}),
        node_id,
    );
    run.workflow_id = workflow.id.unwrap();

    let started_at = Utc::now() - chrono::Duration::seconds(2);
    let mut node_run = node_run(node_id, WorkflowStatus::Waiting);
    node_run.started_at = Some(started_at);

    let api = MockWorkflowApi::with_workflow_run_and_nodes(workflow, run.clone(), vec![node_run]);
    let broker = runinator_broker::in_memory::InMemoryBroker::new();

    process_workflow_run_step(&broker, &api, run).await.unwrap();

    assert_eq!(api.last_run_update().status, WorkflowStatus::TimedOut);
    assert_eq!(api.last_node_update().status, WorkflowStatus::TimedOut);
}

#[tokio::test]
async fn approval_node_times_out() {
    let node_id = "approval_node";
    let workflow = workflow_with_nodes(runinator_models::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": node_id } } },
        {
            "id": node_id,
            "kind": "approval",
            "timeout_seconds": 1
        },
        { "id": "end", "kind": "end" }
    ]));
    let mut run = workflow_run(
        runinator_models::json!({}),
        runinator_models::json!({}),
        node_id,
    );
    run.workflow_id = workflow.id.unwrap();

    let started_at = Utc::now() - chrono::Duration::seconds(2);
    let mut node_run = node_run(node_id, WorkflowStatus::ApprovalRequired);
    node_run.started_at = Some(started_at);

    let api = MockWorkflowApi::with_workflow_run_and_nodes(workflow, run.clone(), vec![node_run]);
    let broker = runinator_broker::in_memory::InMemoryBroker::new();

    process_workflow_run_step(&broker, &api, run).await.unwrap();

    assert_eq!(api.last_run_update().status, WorkflowStatus::TimedOut);
    assert_eq!(api.last_node_update().status, WorkflowStatus::TimedOut);
}

#[tokio::test]
async fn action_node_timeout_publishes_cancel_to_broker() {
    let node_id = "action_node";
    let workflow = workflow_with_nodes(runinator_models::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": node_id } } },
        {
            "id": node_id,
            "kind": "action",
            "action": {
                "provider": "test",
                "function": "test",
                "configuration": {}
            },
            "timeout_seconds": 1
        },
        { "id": "end", "kind": "end" }
    ]));
    let mut run = workflow_run(
        runinator_models::json!({}),
        runinator_models::json!({}),
        node_id,
    );
    run.id = 123;
    run.workflow_id = workflow.id.unwrap();

    let started_at = Utc::now() - chrono::Duration::seconds(2);
    let mut node_run = node_run(node_id, WorkflowStatus::Running);
    node_run.started_at = Some(started_at);

    let api = MockWorkflowApi::with_workflow_run_and_nodes(workflow, run.clone(), vec![node_run]);
    {
        let mut state = api.state.lock().unwrap();
        state.providers = vec![ProviderMetadata {
            name: "test".into(),
            actions: vec![ActionMetadata::new("test", "test")],
            metadata: ProviderRuntimeMetadata::default(),
        }];
    }
    let broker = Arc::new(runinator_broker::in_memory::InMemoryBroker::new());

    process_workflow_run_step(broker.as_ref(), &api, run)
        .await
        .unwrap();

    assert_eq!(api.last_run_update().status, WorkflowStatus::TimedOut);

    // check if control message was published
    let control: runinator_broker::ControlDelivery =
        broker.receive_control("test-consumer").await.unwrap();
    assert_eq!(control.command.workflow_run_id, 123);
    assert_eq!(control.command.kind, ControlKind::Cancel);
}

fn test_error(message: &str) -> SendableError {
    Box::new(RuntimeError::new("scheduler.test".into(), message.into()))
}
