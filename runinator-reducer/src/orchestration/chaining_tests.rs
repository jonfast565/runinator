//! chained triggers driven against the in-memory store: what a terminal run starts, and the one
//! thing it must refuse to start.
//!
//! a chain is the only path that creates a top-level run without going through the schedule, so it
//! is also the only place a disabled workflow could be started behind the operator's back.

use chrono::Utc;
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowRun, WorkflowTrigger, WorkflowTriggerKind,
};
use uuid::Uuid;

use crate::process_ready_node;
use crate::test_support::FakeStore;

const SOURCE_ID: &str = "11111111-1111-1111-1111-111111111111";
const TARGET_ID: &str = "33333333-3333-3333-3333-333333333333";
const RUN_ID: &str = "22222222-2222-2222-2222-222222222222";

fn workflow(id: &str, name: &str, enabled: bool) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": name,
        "version": "1.0.0",
        "enabled": enabled,
        "definition": {
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" },
            ],
        },
    }))
    .expect("workflow definition")
}

fn chained_trigger(source_id: &str, target_name: &str) -> WorkflowTrigger {
    WorkflowTrigger {
        id: Some(Uuid::now_v7()),
        workflow_id: source_id.parse().expect("source id"),
        kind: WorkflowTriggerKind::Chained,
        enabled: true,
        configuration: runinator_models::json!({
            "on": "success",
            "target_workflow": target_name,
        }),
        next_execution: None,
        blackout_start: None,
        blackout_end: None,
        metadata: runinator_models::json!({}),
        created_at: None,
        updated_at: None,
    }
}

fn running_run() -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": RUN_ID,
        "workflow_id": SOURCE_ID,
        "status": "running",
        "active_node_id": "end",
        "parameters": {},
        "state": {},
        "created_at": Utc::now(),
        "started_at": Utc::now(),
        "finished_at": null,
        "message": null,
    }))
    .expect("workflow run")
}

fn ready_end() -> ReadyNodeRecord {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::now_v7(),
        "source_event_id": Uuid::now_v7(),
        "workflow_run_id": RUN_ID,
        "node_id": "end",
        "status": "queued",
        "ready_at": Utc::now(),
        "attempts": 0,
        "created_at": Utc::now(),
        "updated_at": Utc::now(),
    }))
    .expect("ready node")
}

/// seed a source workflow whose success chains to `target`, with the target enabled or not.
fn store(target_enabled: bool) -> FakeStore {
    let db = FakeStore::new();
    db.insert_workflow(workflow(SOURCE_ID, "source", true));
    db.insert_workflow(workflow(TARGET_ID, "target", target_enabled));
    db.insert_trigger(chained_trigger(SOURCE_ID, "target"));
    db.insert_run(running_run());
    db
}

/// how many runs of the target workflow exist. the source's own run is seeded, so counting all runs
/// would not distinguish "the chain fired" from "nothing happened".
fn target_runs(db: &FakeStore) -> usize {
    let target: Uuid = TARGET_ID.parse().expect("target id");
    db.runs()
        .iter()
        .filter(|run| run.workflow_id == target)
        .count()
}

#[tokio::test]
async fn a_succeeded_run_starts_its_chained_target() {
    let db = store(true);
    process_ready_node(&db, &ready_end()).await.expect("drive");
    assert_eq!(target_runs(&db), 1);
}

/// production symptom: disabling a workflow stopped its own schedule but not the chains pointing at
/// it, so a "switched off" workflow kept running whenever its upstream succeeded.
#[tokio::test]
async fn a_disabled_target_is_not_started_by_a_chain() {
    let db = store(false);
    process_ready_node(&db, &ready_end()).await.expect("drive");
    assert_eq!(target_runs(&db), 0);

    // and re-enabling restores the link rather than leaving it permanently burnt.
    let db = store(false);
    db.set_workflow_enabled(TARGET_ID.parse().expect("target id"), true);
    process_ready_node(&db, &ready_end()).await.expect("drive");
    assert_eq!(target_runs(&db), 1);
}
