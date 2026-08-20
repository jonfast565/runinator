//! gate deadline policies and their terminal transitions.

use super::*;
use chrono::Utc;
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::workflow_state::GateState;
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use crate::process_ready_node;
use crate::test_support::FakeStore;

const WORKFLOW_ID: &str = "11111111-1111-1111-1111-111111111111";

fn workflow(timeout_policy: Option<&str>) -> WorkflowDefinition {
    let mut parameters = serde_json::json!({
        "kind": "condition",
        "when": { "value": false },
        "poll_interval": 30,
        "timeout": 60,
    });
    if let Some(policy) = timeout_policy {
        parameters["timeout_policy"] = policy.into();
    }
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "gate timeout test",
        "version": "1.0.0",
        "enabled": true,
        "definition": {
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "gate" } } },
                {
                    "id": "gate",
                    "kind": "gate",
                    "parameters": parameters,
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }
    }))
    .expect("workflow definition")
}

fn run(run_id: Uuid) -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": run_id,
        "workflow_id": WORKFLOW_ID,
        "status": "queued",
        "active_node_id": null,
        "parameters": {},
        "state": {},
        "created_at": Utc::now(),
        "started_at": null,
        "finished_at": null,
        "message": null,
    }))
    .expect("workflow run")
}

fn ready_node(run_id: Uuid, node_id: &str) -> ReadyNodeRecord {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::now_v7(),
        "source_event_id": Uuid::now_v7(),
        "workflow_run_id": run_id,
        "node_id": node_id,
        "status": "queued",
        "ready_at": Utc::now(),
        "attempts": 0,
        "created_at": Utc::now(),
        "updated_at": Utc::now(),
    }))
    .expect("ready node")
}

async fn drive_expired_gate(timeout_policy: Option<&str>) -> (FakeStore, Uuid) {
    let store = FakeStore::new();
    let run_id = Uuid::now_v7();
    store.insert_workflow(workflow(timeout_policy));
    store.insert_run(run(run_id));
    process_ready_node(&store, &ready_node(run_id, "start"))
        .await
        .expect("park gate");

    let gate_run = store.latest_node_run("gate").expect("parked gate run");
    store.set_node_run_state(
        gate_run.id,
        GateState {
            gate_id: None,
            deadline_unix: Some(Utc::now().timestamp() - 1),
            poll_interval: 30,
        }
        .to_wire_value()
        .expect("gate state"),
    );
    process_ready_node(&store, &ready_node(run_id, "gate"))
        .await
        .expect("expire gate");
    (store, run_id)
}

#[tokio::test]
async fn continue_policy_succeeds_when_the_gate_deadline_expires() {
    let (store, run_id) = drive_expired_gate(Some("continue")).await;
    let gate_run = store.latest_node_run("gate").expect("gate run");
    assert_eq!(gate_run.status, WorkflowStatus::Succeeded);
    assert_eq!(gate_run.message.as_deref(), Some("gate_timeout_continued"));
    assert_eq!(
        gate_run.output_json,
        Some(runinator_models::json!({
            "gate_passed": false,
            "gate_timed_out": true,
        }))
    );
    assert_eq!(
        store.run(run_id).expect("run").status,
        WorkflowStatus::Succeeded
    );
}

#[tokio::test]
async fn omitted_policy_keeps_the_existing_timeout_failure() {
    let (store, run_id) = drive_expired_gate(None).await;
    assert_eq!(
        store.latest_node_run("gate").expect("gate run").status,
        WorkflowStatus::TimedOut
    );
    assert_eq!(
        store.run(run_id).expect("run").status,
        WorkflowStatus::TimedOut
    );
}
