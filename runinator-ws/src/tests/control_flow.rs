//! the control-flow node kinds end to end: loop, parallel/join, map (serial, concurrent, fail-fast),
//! try/finally, approval parking, and a subflow whose child wakes its parent.

use super::*;

#[tokio::test]
async fn reducer_runs_loop_node_over_all_items() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "loop-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "loop" } } },
                {
                    "id": "loop",
                    "kind": "loop",
                    "parameters": { "items": ["a", "b", "c"] },
                    "transitions": { "next": { "$node": "body" }, "on_success": { "$node": "done" } }
                },
                { "id": "body", "kind": "output", "transitions": { "on_success": { "$node": "loop" } } },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    let loop_succeeded = nodes
        .iter()
        .filter(|n| n.node_id == "loop" && n.status == WorkflowStatus::Succeeded)
        .count();
    let body_succeeded = nodes
        .iter()
        .filter(|n| n.node_id == "body" && n.status == WorkflowStatus::Succeeded)
        .count();
    // three iterations plus the exhausted visit that exits the loop; the body runs once per item.
    assert_eq!(loop_succeeded, 4);
    assert_eq!(body_succeeded, 3);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_dispatches_loop_body_action_once_per_item() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "loop-action-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "loop" } } },
                {
                    "id": "loop",
                    "kind": "loop",
                    "parameters": { "items": ["a", "b", "c", "d"] },
                    "transitions": { "next": { "$node": "body" }, "on_success": { "$node": "done" } }
                },
                {
                    "id": "body",
                    "kind": "action",
                    "action": { "provider": "console", "function": "run" },
                    "transitions": { "on_success": { "$node": "loop" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    // a re-entered loop body must dispatch a fresh action run per iteration, not reuse the first.
    let body_succeeded = nodes
        .iter()
        .filter(|n| n.node_id == "body" && n.status == WorkflowStatus::Succeeded)
        .count();
    assert_eq!(body_succeeded, 4);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_fans_out_parallel_branches_and_joins() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "parallel-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "fork" } } },
                {
                    "id": "fork",
                    "kind": "parallel",
                    "parameters": { "branches": [{ "$node": "a" }, { "$node": "b" }] },
                    "transitions": {}
                },
                {
                    "id": "a",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute" },
                    "transitions": { "on_success": { "$node": "join" } }
                },
                {
                    "id": "b",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute" },
                    "transitions": { "on_success": { "$node": "join" } }
                },
                {
                    "id": "join",
                    "kind": "join",
                    "parameters": { "wait_for": [{ "$node": "a" }, { "$node": "b" }], "mode": "all" },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    for branch in ["a", "b", "join"] {
        assert!(
            nodes
                .iter()
                .any(|n| n.node_id == branch && n.status == WorkflowStatus::Succeeded),
            "branch {branch} should have succeeded"
        );
    }
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_maps_items_through_target_node() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "map-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
                {
                    "id": "map",
                    "kind": "map",
                    "parameters": { "items": [1, 2], "target": { "$node": "each" } },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "each", "kind": "output", "transitions": { "on_success": { "$node": "map" } } },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    // each item runs the body in its own child run; the map gathers their outputs in order.
    let outputs = map_node_outputs(&db, run_id).await;
    assert_eq!(outputs.len(), 2);
    let _ = std::fs::remove_file(path);
}

/// fetch the ordered per-item outputs recorded on a run's `map` node.
async fn map_node_outputs(db: &SqliteDb, run_id: Uuid) -> Vec<Value> {
    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    nodes
        .iter()
        .filter(|n| n.node_id == "map" && n.status == WorkflowStatus::Succeeded)
        .find_map(|n| n.output_json.as_ref())
        .and_then(|output| output.get("outputs"))
        .and_then(|outputs| outputs.as_array().cloned())
        .unwrap_or_default()
}

#[tokio::test]
async fn reducer_maps_items_concurrently_in_order() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "map-concurrent-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
                {
                    "id": "map",
                    "kind": "map",
                    "parameters": {
                        "items": [10, 20, 30, 40, 50],
                        "target": { "$node": "each" },
                        "concurrency": 3
                    },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                {
                    "id": "each",
                    "kind": "output",
                    "parameters": { "data": { "$ref": { "node": "map", "output": ["item"] } } },
                    "transitions": { "on_success": { "$node": "map" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    // five items fanned out three-at-a-time still gather in item order.
    let outputs = map_node_outputs(&db, run_id).await;
    let items: Vec<i64> = outputs
        .iter()
        .filter_map(|output| output.get("data").and_then(Value::as_i64))
        .collect();
    assert_eq!(items, vec![10, 20, 30, 40, 50]);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_map_fails_fast_when_item_fails() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "map-fail-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
                {
                    "id": "map",
                    "kind": "map",
                    "parameters": {
                        "items": [1, 2, 3],
                        "target": { "$node": "work" },
                        "concurrency": 3
                    },
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
        }),
    )
    .await;

    // drive the fan-out, then fail the first item's action and succeed the rest.
    let mut failed_one = false;
    let mut run = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap()
        .0;
    for _ in 0..64 {
        drain_ready_nodes(&db).await;
        run = crate::repository::fetch_workflow_run(&db, run_id)
            .await
            .unwrap()
            .unwrap()
            .0;
        if run.status.is_terminal() {
            break;
        }
        let dispatches = db.fetch_pending_action_dispatches(50).await.unwrap();
        if dispatches.is_empty() {
            break;
        }
        for dispatch in dispatches {
            db.mark_action_dispatch_published(dispatch.id)
                .await
                .unwrap();
            let status = if failed_one {
                WorkflowStatus::Succeeded
            } else {
                failed_one = true;
                WorkflowStatus::Failed
            };
            let event = WorkflowResultEvent::status(&dispatch.command, status, None, None);
            crate::repository::apply_workflow_result_event(&db, &event)
                .await
                .unwrap();
        }
    }
    // a single failed item fails the whole map (no on_failure routing here).
    assert_eq!(run.status, WorkflowStatus::Failed);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_try_node_runs_body_then_finally() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "try-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "try" } } },
                {
                    "id": "try",
                    "kind": "try",
                    "parameters": { "body": { "$node": "body" }, "finally": { "$node": "cleanup" } },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "body", "kind": "output", "transitions": { "on_success": { "$node": "try" } } },
                { "id": "cleanup", "kind": "output", "transitions": { "on_success": { "$node": "try" } } },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    for stage in ["body", "cleanup"] {
        assert!(
            nodes
                .iter()
                .any(|n| n.node_id == stage && n.status == WorkflowStatus::Succeeded),
            "{stage} should have run"
        );
    }
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_parks_approval_then_resolution_wakes_and_completes() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "approval-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "gate" } } },
                {
                    "id": "gate",
                    "kind": "approval",
                    "parameters": { "prompt": "approve?" },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    // the approval node parks the run waiting for an external decision.
    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::ApprovalRequired);
    assert_eq!(run.active_node_id.as_deref(), Some("gate"));

    // resolve the approval the way the api handler would.
    let approvals = db
        .fetch_automation_records("approval_requests".into(), Some(run_id), None)
        .await
        .unwrap();
    assert_eq!(approvals.len(), 1);
    let approval_id = approvals[0]
        .get("id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
        .unwrap();
    crate::repository::resolve_approval(&db, approval_id, true, None, None, None)
        .await
        .unwrap();

    // resolution should have enqueued a ready node; draining now completes the run.
    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_subflow_waits_for_child_and_child_terminal_wakes_parent() {
    let (db, path) = test_db().await;

    // child workflow that completes on its own.
    let mut child = workflow(None, "child-flow");
    child.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let child = db.upsert_workflow(&child).await.unwrap();
    let child_id = child.id.unwrap();

    // parent that launches the child as a waiting subflow.
    let mut parent = workflow(None, "parent-flow");
    parent.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "sub" } } },
            {
                "id": "sub",
                "kind": "subflow",
                "subflow_id": child_id,
                "subflow": { "type": "wait" },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let parent = db.upsert_workflow(&parent).await.unwrap();
    let parent_run = crate::repository::create_workflow_run(
        &db,
        parent.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    // draining drives the parent to launch + the child to completion; the terminal child wakes the
    // parent's subflow node, which then transitions to its end.
    drain_ready_nodes(&db).await;
    let (run, _) = crate::repository::fetch_workflow_run(&db, parent_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        run.status,
        WorkflowStatus::Succeeded,
        "parent run should complete after child finishes, got {:?}",
        run.status
    );
    let _ = std::fs::remove_file(path);
}
