//! chained triggers: firing a target on a source outcome, honouring the status selector, and the
//! depth bound that stops a self-chain.

use super::*;

/// build a `managed_by: wdl` chained trigger for `source_id` targeting `target_name` on `on`.
fn chained_trigger(source_id: Uuid, target_name: &str, on: &str) -> WorkflowTrigger {
    WorkflowTrigger {
        id: None,
        workflow_id: source_id,
        kind: WorkflowTriggerKind::Chained,
        enabled: true,
        configuration: json!({ "on": on, "target_workflow": target_name, "parameters": {} }),
        next_execution: None,
        blackout_start: None,
        blackout_end: None,
        metadata: json!({ "managed_by": "wdl" }),
        created_at: None,
        updated_at: None,
    }
}

#[tokio::test]
async fn reducer_chains_target_on_source_success() {
    let (db, path) = test_db().await;

    let target = db
        .upsert_workflow(&workflow(None, "chain-target-b"))
        .await
        .unwrap();
    let target_id = target.id.unwrap();
    let source = db
        .upsert_workflow(&workflow(None, "chain-source-a"))
        .await
        .unwrap();
    let source_id = source.id.unwrap();
    db.upsert_workflow_trigger(&chained_trigger(source_id, "chain-target-b", "success"))
        .await
        .unwrap();

    let source_run = crate::repository::create_workflow_run(
        &db,
        source_id,
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    drain_ready_nodes(&db).await;

    let (run, _) = crate::repository::fetch_workflow_run(&db, source_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    // the target started exactly once, provenance-stamped as chained.
    let target_runs = db
        .fetch_workflow_runs_for_workflow(target_id)
        .await
        .unwrap();
    assert_eq!(target_runs.len(), 1, "expected one chained target run");
    assert_eq!(
        target_runs[0].trigger_source_kind,
        Some(runinator_models::replicas::TriggerSourceKind::Chained)
    );
    assert_eq!(
        target_runs[0].trigger_metadata.get("chained_from_run_id"),
        Some(&json!(source_run.id))
    );

    // re-draining the same terminal run must not start a duplicate (firing dedupe).
    drain_ready_nodes(&db).await;
    let target_runs = db
        .fetch_workflow_runs_for_workflow(target_id)
        .await
        .unwrap();
    assert_eq!(
        target_runs.len(),
        1,
        "chain must be exactly-once per source run"
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_chain_respects_status_selector() {
    let (db, path) = test_db().await;

    let target = db
        .upsert_workflow(&workflow(None, "chain-target-fail"))
        .await
        .unwrap();
    let target_id = target.id.unwrap();
    let source = db
        .upsert_workflow(&workflow(None, "chain-source-ok"))
        .await
        .unwrap();
    let source_id = source.id.unwrap();
    // only chains on failure; the source succeeds, so nothing should start.
    db.upsert_workflow_trigger(&chained_trigger(source_id, "chain-target-fail", "failure"))
        .await
        .unwrap();

    let source_run = crate::repository::create_workflow_run(
        &db,
        source_id,
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    drain_ready_nodes(&db).await;

    let (run, _) = crate::repository::fetch_workflow_run(&db, source_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    let target_runs = db
        .fetch_workflow_runs_for_workflow(target_id)
        .await
        .unwrap();
    assert!(
        target_runs.is_empty(),
        "on_failure trigger must not fire when the source succeeds"
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_chain_depth_is_bounded() {
    let (db, path) = test_db().await;

    // a workflow that chains to itself would loop forever without the depth cap.
    let a = db
        .upsert_workflow(&workflow(None, "chain-loop-a"))
        .await
        .unwrap();
    let a_id = a.id.unwrap();
    db.upsert_workflow_trigger(&chained_trigger(a_id, "chain-loop-a", "success"))
        .await
        .unwrap();

    crate::repository::create_workflow_run(&db, a_id, json!({}), false, None, Default::default())
        .await
        .unwrap();
    drain_ready_nodes(&db).await;

    // depth 0 (original) chains through depth 32, which is the cap and stops: 33 runs total.
    let runs = db.fetch_workflow_runs_for_workflow(a_id).await.unwrap();
    assert_eq!(runs.len(), 33, "self-chain must stop at MAX_CHAIN_DEPTH");
    let _ = std::fs::remove_file(path);
}
