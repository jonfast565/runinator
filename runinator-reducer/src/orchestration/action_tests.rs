//! unavailable-worker action liveness and terminal timeout behavior.

use chrono::{Duration, Utc};
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use crate::process_ready_node;
use crate::test_support::FakeStore;

const WORKFLOW_ID: &str = "11111111-1111-1111-1111-111111111111";

fn ready(run_id: Uuid, node_id: &str) -> ReadyNodeRecord {
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

#[tokio::test]
async fn unavailable_worker_timeout_settles_once_without_execution_retry() {
    let store = FakeStore::new();
    let workflow: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "unavailable worker timeout",
        "version": "1.0.0",
        "enabled": true,
        "definition": {
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "sync" } } },
                {
                    "id": "sync",
                    "kind": "action",
                    "action": {
                        "provider": "aws",
                        "function": "sync",
                        "required_labels": { "runner": "creds-sync" }
                    },
                    "timeout_seconds": 1,
                    "retry": { "max_attempts": 5, "backoff_base_seconds": 1 },
                    "transitions": { "on_timeout": { "$node": "failed" } }
                },
                { "id": "failed", "kind": "fail" },
                { "id": "done", "kind": "end" }
            ]
        }
    }))
    .expect("workflow");
    let run_id = Uuid::now_v7();
    let run: WorkflowRun = serde_json::from_value(serde_json::json!({
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
    .expect("run");
    store.insert_workflow(workflow);
    store.insert_run(run);

    process_ready_node(&store, &ready(run_id, "start"))
        .await
        .expect("initial drive");
    let parked = store
        .node_runs()
        .into_iter()
        .find(|node_run| node_run.workflow_run_id == run_id && node_run.node_id == "sync")
        .expect("parked action");
    assert_eq!(parked.status, WorkflowStatus::Waiting);
    assert!(
        store.dispatches().is_empty(),
        "no action attempt was dispatched"
    );

    store.age_node_run(parked.id, Duration::seconds(2));
    let timeout_drive = store
        .ready_nodes()
        .into_iter()
        .find(|ready| ready.workflow_run_id == run_id && ready.node_id == "sync")
        .expect("timeout or compatibility poll");
    process_ready_node(&store, &timeout_drive)
        .await
        .expect("timeout drive");

    let sync_runs = store
        .node_runs()
        .into_iter()
        .filter(|node_run| node_run.workflow_run_id == run_id && node_run.node_id == "sync")
        .collect::<Vec<_>>();
    assert_eq!(
        sync_runs.len(),
        1,
        "the parked node is not recreated for a retry"
    );
    assert_eq!(sync_runs[0].status, WorkflowStatus::TimedOut);
    assert_ne!(
        sync_runs[0].transition_reason.as_deref(),
        Some("retry_queued")
    );
    assert_eq!(
        store.run(run_id).expect("run").status,
        WorkflowStatus::Failed
    );

    process_ready_node(&store, &timeout_drive)
        .await
        .expect("stale duplicate drive");
    assert_eq!(
        store
            .node_runs()
            .into_iter()
            .filter(|node_run| node_run.workflow_run_id == run_id && node_run.node_id == "sync")
            .count(),
        1,
        "a stale poll cannot rearm attempt 1 after the run is terminal"
    );
}

#[tokio::test]
async fn foreign_compute_dispatch_carries_its_declared_output_type() {
    let store = FakeStore::new();
    let workflow: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "typed foreign compute",
        "version": "1.0.0",
        "enabled": true,
        "definition": {
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "code" } } },
                {
                    "id": "code",
                    "kind": "action",
                    "action": {
                        "provider": "std",
                        "function": "code",
                        "configuration": {
                            "language": "python",
                            "source": "def main(context):\n    return 42\n"
                        }
                    },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ],
            "metadata": {
                "rexrap": { "type_hints": { "code": { "type": "integer" } } }
            }
        }
    }))
    .expect("workflow");
    let run_id = Uuid::now_v7();
    let run: WorkflowRun = serde_json::from_value(serde_json::json!({
        "id": run_id,
        "workflow_id": WORKFLOW_ID,
        "status": "queued",
        "active_node_id": null,
        "parameters": {},
        "state": {},
        "created_at": Utc::now(),
        "started_at": null,
        "finished_at": null,
        "message": null
    }))
    .expect("run");
    store.insert_workflow(workflow);
    store.insert_run(run);

    process_ready_node(&store, &ready(run_id, "start"))
        .await
        .expect("foreign compute drive");

    let dispatch = store.dispatches().into_iter().next().expect("dispatch");
    assert_eq!(
        dispatch.command.parameters.get("expected_output_type"),
        Some(&runinator_models::json!({ "type": "integer" }))
    );
}
