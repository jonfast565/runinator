//! driving ready nodes through the reducer against a real database: dispatch, in-process compute,
//! retry backoff, and the recovery paths for a timed-out or disconnected executor.

use super::*;

/// the requested-interrupt path end to end against a real database: the endpoint records the ask,
/// the reducer picks it up on the next drive, and the handler region runs on its own cursor.
#[tokio::test]
async fn a_requested_interrupt_runs_its_handler_region() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "requested-interrupt");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "hold" } } },
            {
                "id": "hold", "kind": "signal", "parameters": { "name": "later" },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" },
            {
                "id": "note", "kind": "transform",
                "parameters": { "bindings": { "touched": true } },
                "transitions": { "on_success": { "$node": "handed_back" } }
            },
            { "id": "handed_back", "kind": "resume" }
        ],
        "metadata": { "interrupts": [{ "on": "external", "handler": "note" }] }
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    drain_ready_nodes(&db).await;
    let (parked, _) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parked.active_node_id.as_deref(), Some("hold"));

    let response = crate::repository::request_run_interrupt(
        &db,
        run.id,
        runinator_models::interrupt::InterruptSource::External,
        json!({ "reason": "credentials rotated" }),
        None,
    )
    .await
    .unwrap();
    assert!(response.success, "{}", response.message);

    drain_ready_nodes(&db).await;

    let (_, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        nodes.iter().any(|node| node.node_id == "note"),
        "the handler region ran: {:?}",
        nodes.iter().map(|n| &n.node_id).collect::<Vec<_>>()
    );
    let (settled, _) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    let state = runinator_models::workflow_state::WorkflowRunState::from_state(&settled.state);
    assert!(
        state.pending_interrupts.is_empty(),
        "the request is consumed by the drive that raised it"
    );
    assert!(
        state.cursors.iter().all(|cursor| !cursor.is_suspended()),
        "control came back to the parked signal"
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn ready_node_processing_reduces_start_to_action_dispatch() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "ready-reducer");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
            {
                "id": "run",
                "kind": "action",
                "action": {
                    "provider": "test",
                    "function": "execute",
                    "configuration": { "message": "hello" }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    let ready = crate::repository::claim_ready_nodes(
        &db,
        "scheduler-a".into(),
        chrono::Utc::now() + chrono::Duration::seconds(30),
        10,
    )
    .await
    .unwrap();
    assert_eq!(ready.len(), 1);

    crate::repository::complete_ready_node(&db, ready[0].id, "scheduler-a".into(), None)
        .await
        .unwrap();

    let (updated, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, WorkflowStatus::Running);
    assert_eq!(updated.active_node_id.as_deref(), Some("run"));
    assert!(
        nodes
            .iter()
            .any(|node| node.node_id == "run" && node.status == WorkflowStatus::Running)
    );
    let dispatches = db.fetch_pending_action_dispatches(10).await.unwrap();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].command.node_id, "run");

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn output_nodes_write_automation_events_for_the_events_tab() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "output-events");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "output" } } },
            {
                "id": "output",
                "kind": "output",
                "parameters": {
                    "event_type": "workflow.routed",
                    "data": { "ok": true, "count": 1 }
                },
                "transitions": { "next": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    let ready = crate::repository::claim_ready_nodes(
        &db,
        "scheduler-a".into(),
        chrono::Utc::now() + chrono::Duration::seconds(30),
        10,
    )
    .await
    .unwrap();
    assert_eq!(ready.len(), 1);

    crate::repository::complete_ready_node(&db, ready[0].id, "scheduler-a".into(), None)
        .await
        .unwrap();

    let events = db
        .fetch_automation_records("automation_events".into(), Some(run.id), None)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    let metadata = event
        .get("metadata")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        event.get("event_type").and_then(Value::as_str),
        Some("workflow.routed")
    );
    assert_eq!(
        event.get("provider").and_then(Value::as_str),
        Some("runinator")
    );
    assert_eq!(
        event.get("status").and_then(Value::as_str),
        Some("output_recorded")
    );
    assert_eq!(
        metadata
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("ok"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn pure_compute_node_reruns_in_loop_body() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "loop-compute");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
            {
                "id": "each",
                "kind": "loop",
                "parameters": { "items": { "$ref": { "input": ["xs"] } } },
                "max_iterations": 10,
                "transitions": {
                    "next": { "$node": "double" },
                    "on_success": { "$node": "done" }
                }
            },
            {
                "id": "double",
                "kind": "action",
                "action": {
                    "provider": "std",
                    "function": "run",
                    "configuration": {
                        "program": [
                            { "$return": { "$mul": [{ "$ref": { "node": "each", "output": ["item"] } }, 2] } }
                        ]
                    }
                },
                "transitions": { "on_success": { "$node": "each" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({ "xs": [1, 2, 3] }),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    drain_ready_nodes(&db).await;

    let (updated, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, WorkflowStatus::Succeeded);
    // the compute body ran once per item, re-creating a fresh node run each iteration.
    let runs = nodes
        .iter()
        .filter(|node| node.node_id == "double" && node.status == WorkflowStatus::Succeeded)
        .count();
    assert_eq!(runs, 3, "compute body should run once per loop item");
    // and never dispatched to a worker.
    assert!(
        db.fetch_pending_action_dispatches(10)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn pure_compute_node_reduces_in_process_without_dispatch() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "pure-compute");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "calc" } } },
            {
                "id": "calc",
                "kind": "action",
                "action": {
                    "provider": "std",
                    "function": "run",
                    "configuration": {
                        "program": [
                            { "$let": "total", "value": { "$add": [{ "$ref": { "input": ["a"] } }, 3] } },
                            { "$return": { "total": { "$ref": { "let": ["total"] } } } }
                        ]
                    }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({ "a": 4 }),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    drain_ready_nodes(&db).await;

    let (updated, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    // the pure compute node reduced in-process and the run reached the end node.
    assert_eq!(updated.status, WorkflowStatus::Succeeded);
    let calc = nodes.iter().find(|node| node.node_id == "calc").unwrap();
    assert_eq!(calc.status, WorkflowStatus::Succeeded);
    assert_eq!(calc.output_json, Some(json!({ "total": 7 })));
    // no worker dispatch was enqueued for the pure node.
    assert!(
        db.fetch_pending_action_dispatches(10)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn compute_goto_sets_active_node() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "compute-goto");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "gate" } } },
            {
                "id": "gate",
                "kind": "action",
                "action": {
                    "provider": "std",
                    "function": "run",
                    "configuration": {
                        "program": [
                            { "$if": { "value": { "$ref": { "input": ["x"] } }, "less_than": 0 },
                              "then": [ { "$goto": "fail" } ],
                              "else": [] },
                            { "$return": "ok" }
                        ]
                    }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "fail", "kind": "fail" },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({ "x": -1 }),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    drain_ready_nodes(&db).await;

    let (updated, _) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    // goto fail routed the run to the fail node, ending the run as failed.
    assert_eq!(updated.status, WorkflowStatus::Failed);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn action_failure_schedules_retry_with_backoff() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "action-retry");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
            {
                "id": "run",
                "kind": "action",
                "action": {
                    "provider": "test",
                    "function": "execute",
                    "configuration": { "message": "hello" }
                },
                // pinned rather than left to the model default, so the window asserted below
                // stays meaningful if that default ever moves.
                "retry": { "max_attempts": 3, "backoff_base_seconds": 1 },
                "transitions": { "on_failure": { "$node": "failed" } }
            },
            { "id": "failed", "kind": "fail" },
            { "id": "end", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();
    let event = WorkflowResultEvent::status(&dispatch.command, WorkflowStatus::Failed, None, None);

    // `ready_at` is `backoff` past the clock reading taken when the retry is scheduled, so bracket
    // that moment from both sides. asserting `ready_at > Utc::now()` at the end instead gave the
    // whole tail of the test a budget equal to the backoff itself, and a loaded machine loses it.
    let before_schedule = chrono::Utc::now();
    crate::repository::apply_workflow_result_event(&db, &event)
        .await
        .unwrap();

    drain_ready_nodes(&db).await;
    let after_schedule = chrono::Utc::now();

    let (updated, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, WorkflowStatus::Waiting);
    assert_eq!(updated.active_node_id.as_deref(), Some("run"));
    let run_node = nodes.iter().find(|node| node.node_id == "run").unwrap();
    assert_eq!(run_node.status, WorkflowStatus::Queued);
    assert_eq!(run_node.attempt, 1);
    let retry_ready = db
        .fetch_pending_ready_nodes(chrono::Utc::now(), 10)
        .await
        .unwrap()
        .into_iter()
        .find(|ready| ready.workflow_run_id == run.id && ready.node_id == "run")
        .expect("retry ready node is pending");
    // the backoff was applied, and applied once: a zero delay or a doubled one both fail here,
    // where the old `> now()` check passed for any future instant at all. compared at whole-second
    // resolution because that is what the column stores — a `ready_at` of 10.653 persists as 10 —
    // and both bounds are derived from the same bracket, so a slow machine moves them together
    // instead of eating into a fixed margin.
    let backoff = chrono::Duration::seconds(1);
    let window = (before_schedule + backoff).timestamp()..=(after_schedule + backoff).timestamp();
    assert!(
        window.contains(&retry_ready.ready_at.timestamp()),
        "retry ready_at {} outside the expected backoff window [{}, {}]",
        retry_ready.ready_at,
        before_schedule + backoff,
        after_schedule + backoff
    );
    assert!(
        db.fetch_pending_action_dispatches(10)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn action_retry_republishes_dispatch_after_backoff() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "action-retry-redispatch",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
                {
                    "id": "run",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute", "configuration": {} },
                    "retry": { "max_attempts": 3 },
                    "transitions": { "on_failure": { "$node": "failed" } }
                },
                { "id": "failed", "kind": "fail" },
                { "id": "end", "kind": "end" }
            ]
        }),
    )
    .await;

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();
    let event = WorkflowResultEvent::status(&dispatch.command, WorkflowStatus::Failed, None, None);
    crate::repository::apply_workflow_result_event(&db, &event)
        .await
        .unwrap();
    drain_ready_nodes(&db).await;

    // wait out the first retry backoff, then drive the retry ready node to its re-dispatch. this
    // polls rather than sleeping a flat span: a fixed sleep has to guess how much longer than the
    // backoff the machine will need, and a loaded machine beats the guess. the deadline is what
    // fails the test if the retry genuinely never comes due.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let pending = loop {
        drain_ready_nodes(&db).await;
        let pending = db.fetch_pending_action_dispatches(10).await.unwrap();
        if !pending.is_empty() {
            break pending;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "retry dispatch never came due"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };

    // the retried attempt must publish a fresh outbox row; reusing the first attempt's dedupe key
    // would collide with the already-published row and park the run in `running` forever.
    assert_eq!(pending.len(), 1, "retry must enqueue a fresh dispatch");
    assert_eq!(pending[0].command.attempt, 2);
    assert_ne!(pending[0].dedupe_key, dispatch.dedupe_key);
    let (run, _) = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, WorkflowStatus::Running);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn duplicate_terminal_result_event_still_enqueues_drive() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "duplicate-result-drive",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
                {
                    "id": "run",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute", "configuration": {} },
                    "transitions": { "next": { "$node": "end" } }
                },
                { "id": "end", "kind": "end" }
            ]
        }),
    )
    .await;

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();
    let event = WorkflowResultEvent::status(
        &dispatch.command,
        WorkflowStatus::Succeeded,
        Some(json!({ "ok": true })),
        None,
    );
    assert!(
        crate::repository::apply_workflow_result_event(&db, &event)
            .await
            .unwrap()
    );
    drain_ready_nodes(&db).await;
    let (run, _) = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    // a redelivered duplicate can follow a crash that lost the first drive enqueue; it must still
    // enqueue a drive even though the event itself is not re-applied.
    assert!(
        !crate::repository::apply_workflow_result_event(&db, &event)
            .await
            .unwrap()
    );
    let pending = db
        .fetch_pending_ready_nodes(chrono::Utc::now(), 10)
        .await
        .unwrap();
    assert!(
        pending
            .iter()
            .any(|node| node.workflow_run_id == run_id && node.node_id == "run")
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn action_node_timeout_recovers_parked_run() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "action-timeout",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
                {
                    "id": "run",
                    "kind": "action",
                    "timeout_seconds": 1,
                    "action": { "provider": "test", "function": "execute", "configuration": {} },
                    "transitions": { "next": { "$node": "end" } }
                },
                { "id": "end", "kind": "end" }
            ]
        }),
    )
    .await;

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();

    // no worker result ever arrives; the armed timeout wake must settle the parked node. the margin
    // over the node's 1s timeout is generous on purpose: at 1500ms this test failed intermittently
    // when the workspace suite ran in parallel and the sleep lost its slack to scheduler pressure.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    drain_ready_nodes(&db).await;

    let (run, nodes) = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap();
    let node_run = nodes.iter().find(|node| node.node_id == "run").unwrap();
    assert_eq!(node_run.status, WorkflowStatus::TimedOut);
    assert_eq!(run.status, WorkflowStatus::TimedOut);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn action_node_fails_promptly_when_its_executing_worker_disconnects() {
    use runinator_models::replicas::{ReplicaKind, ReplicaRegistrationRequest};

    let (db, path) = test_db().await;
    // no timeout_seconds: only the armed liveness poll can catch the dead executor.
    let run_id = seed_run(
        &db,
        "executor-disconnect",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
                {
                    "id": "run",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute", "configuration": {} },
                    "transitions": { "next": { "$node": "end" } }
                },
                { "id": "end", "kind": "end" }
            ]
        }),
    )
    .await;

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();

    // a worker claimed the execution, then went offline without releasing the claim (crash or
    // grace-period abort), so the reducer must treat the holder as disconnected.
    let node_run_id = dispatch.command.workflow_node_run_id;
    let runtime_id = Uuid::new_v4().to_string();
    let dead_replica = db
        .register_replica(
            ReplicaRegistrationRequest {
                replica_type: ReplicaKind::Worker,
                instance_id: "doomed-worker".into(),
                runtime_id: runtime_id.clone(),
                display_name: None,
                host: None,
                port: None,
                base_path: None,
                version: None,
                attributes: runinator_models::json!({}),
            },
            None,
            &AuthContext {
                principal_id: None,
                is_admin: true,
                kind: PrincipalKind::User,
                org_id: None,
                org_role: None,
            },
        )
        .await
        .unwrap()
        .replica_id;
    assert!(
        db.claim_workflow_node_run_executor(
            node_run_id,
            dead_replica,
            chrono::Utc::now(),
            chrono::Utc::now() - chrono::Duration::seconds(60),
            chrono::Utc::now() - chrono::Duration::seconds(30),
        )
        .await
        .unwrap()
    );
    db.mark_replica_offline(dead_replica, runtime_id)
        .await
        .unwrap();

    // the armed liveness poll is 15 seconds out; fire an equivalent due wake instead of sleeping.
    db.enqueue_ready_node(
        runinator_models::orchestration::NewOrchestrationEvent::new(
            run_id,
            Some("run".into()),
            "dispatch_liveness_poll",
            json!({ "node_id": "run" }),
        ),
        "run".into(),
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    drain_ready_nodes(&db).await;

    let (run, nodes) = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap();
    let node_run = nodes.iter().find(|node| node.node_id == "run").unwrap();
    assert_eq!(node_run.status, WorkflowStatus::TimedOut);
    // the dead worker's claim must be released, or the retry this schedules would be dropped as a
    // duplicate delivery by whichever worker picks it up.
    assert!(node_run.current_executor_replica_id.is_none());
    assert_eq!(run.status, WorkflowStatus::TimedOut);

    let _ = std::fs::remove_file(path);
}
