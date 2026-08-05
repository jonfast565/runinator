//! correlation keys: stamping one from run metadata, and waking an awaiting run only on a terminal
//! run whose key matches.

use super::*;

/// a workflow whose runs stamp a correlation key from `metadata.correlation` as they start.
fn correlating_workflow(name: &str, correlation: Value) -> WorkflowDefinition {
    let mut workflow = workflow(None, name);
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "metadata": { "correlation": correlation },
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    workflow
}

/// a workflow that parks on `await workflow "<target>" key "<key>"`.
fn awaiting_workflow(name: &str, target: &str, key: &str) -> WorkflowDefinition {
    let mut workflow = workflow(None, name);
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "aw" } } },
            {
                "id": "aw",
                "kind": "await_run",
                "parameters": { "workflow": target, "key": key, "mode": "all" },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    workflow
}

#[tokio::test]
async fn workflow_run_correlation_key_stamped_from_metadata() {
    let (db, path) = test_db().await;
    let a = db
        .upsert_workflow(&correlating_workflow("corr-source", json!("batch-42")))
        .await
        .unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        a.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    drain_ready_nodes(&db).await;
    let (run, _) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    assert_eq!(run.correlation_key.as_deref(), Some("batch-42"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_await_workflow_wakes_on_matching_terminal_run() {
    let (db, path) = test_db().await;
    let target = db
        .upsert_workflow(&workflow(None, "await-target-a"))
        .await
        .unwrap();
    let target_id = target.id.unwrap();
    let waiter = db
        .upsert_workflow(&awaiting_workflow("await-waiter-b", "await-target-a", "b1"))
        .await
        .unwrap();

    // the waiter parks on the await node with no matching run yet.
    let waiter_run = crate::repository::create_workflow_run(
        &db,
        waiter.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    drain_ready_nodes(&db).await;
    let (parked, _) = crate::repository::fetch_workflow_run(&db, waiter_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parked.status, WorkflowStatus::Waiting, "waiter should park");

    // a matching-correlation run of the target then completes and wakes the waiter.
    let target_run = crate::repository::create_workflow_run(
        &db,
        target_id,
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    db.set_run_correlation_key(target_run.id, "b1".into())
        .await
        .unwrap();
    drain_ready_nodes(&db).await;

    let (done, _) = crate::repository::fetch_workflow_run(&db, waiter_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        done.status,
        WorkflowStatus::Succeeded,
        "waiter should complete once a matching run finishes, got {:?}",
        done.status
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_await_workflow_ignores_correlation_mismatch() {
    let (db, path) = test_db().await;
    let target = db
        .upsert_workflow(&workflow(None, "await-target-c"))
        .await
        .unwrap();
    let target_id = target.id.unwrap();
    let waiter = db
        .upsert_workflow(&awaiting_workflow("await-waiter-d", "await-target-c", "b1"))
        .await
        .unwrap();
    let waiter_run = crate::repository::create_workflow_run(
        &db,
        waiter.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    drain_ready_nodes(&db).await;

    // a target run with a different correlation key completes; the waiter must keep waiting.
    let target_run = crate::repository::create_workflow_run(
        &db,
        target_id,
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    db.set_run_correlation_key(target_run.id, "other".into())
        .await
        .unwrap();
    drain_ready_nodes(&db).await;

    let (still_waiting, _) = crate::repository::fetch_workflow_run(&db, waiter_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        still_waiting.status,
        WorkflowStatus::Waiting,
        "waiter must not complete on a non-matching correlation, got {:?}",
        still_waiting.status
    );
    let _ = std::fs::remove_file(path);
}
