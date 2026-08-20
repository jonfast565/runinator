//! fifo mutex acquisition, cursor ownership, and durable poll recovery.

use chrono::Utc;
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use super::mutex::MutexOps;
use crate::process_ready_node;
use crate::test_support::FakeStore;

const WORKFLOW_ID: &str = "11111111-1111-1111-1111-111111111111";

fn workflow(nodes: serde_json::Value) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "mutex test",
        "version": "1.0.0",
        "enabled": true,
        "definition": { "start": "start", "nodes": nodes }
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

fn held_section() -> serde_json::Value {
    serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "lock" } } },
        {
            "id": "lock",
            "kind": "mutex",
            "parameters": {
                "name": "deploy",
                "poll_interval_seconds": 5,
                "hold_timeout_seconds": 1
            },
            "transitions": { "on_success": { "$node": "hold" } }
        },
        {
            "id": "hold", "kind": "signal", "parameters": { "name": "release-me" },
            "transitions": { "on_success": { "$node": "done" } }
        },
        { "id": "done", "kind": "end" }
    ])
}

fn node_run(
    store: &FakeStore,
    run_id: Uuid,
    node_id: &str,
) -> runinator_models::workflows::WorkflowNodeRun {
    store
        .node_runs()
        .into_iter()
        .rfind(|node_run| node_run.workflow_run_id == run_id && node_run.node_id == node_id)
        .expect("node run")
}

async fn start(store: &FakeStore, run_id: Uuid) {
    store.insert_run(run(run_id));
    process_ready_node(store, &ready(run_id, "start"))
        .await
        .expect("drive run");
}

#[tokio::test]
async fn handoff_is_fifo_across_three_waiters() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(held_section()));
    let holder = Uuid::now_v7();
    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    let third = Uuid::now_v7();

    for run_id in [holder, first, second, third] {
        start(&store, run_id).await;
    }

    for (current, expected) in [(holder, first), (first, second), (second, third)] {
        store.settle_run(current, WorkflowStatus::Canceled);
        let before_release = store.ready_nodes().len();
        MutexOps::new(&store)
            .release_run_mutexes(current)
            .await
            .expect("release fifo holder");
        let immediate = store
            .ready_nodes()
            .get(before_release)
            .cloned()
            .expect("immediate fifo wake");
        assert_eq!(
            immediate.cursor_id,
            node_run(&store, expected, "lock").cursor_id,
            "handoff must follow original enqueue order"
        );
        process_ready_node(&store, &immediate)
            .await
            .expect("drive next fifo holder");
        assert_eq!(
            node_run(&store, expected, "lock").status,
            WorkflowStatus::Succeeded
        );
    }
}

#[tokio::test]
async fn terminal_holder_hands_off_to_oldest_active_waiter() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(held_section()));
    let holder = Uuid::now_v7();
    let canceled_waiter = Uuid::now_v7();
    let successor = Uuid::now_v7();

    start(&store, holder).await;
    start(&store, canceled_waiter).await;
    start(&store, successor).await;
    assert_eq!(
        node_run(&store, holder, "lock").status,
        WorkflowStatus::Succeeded
    );
    assert_eq!(
        node_run(&store, canceled_waiter, "lock").status,
        WorkflowStatus::Waiting
    );
    assert_eq!(
        node_run(&store, successor, "lock").status,
        WorkflowStatus::Waiting
    );

    store.settle_run(canceled_waiter, WorkflowStatus::Canceled);
    MutexOps::new(&store)
        .release_run_mutexes(canceled_waiter)
        .await
        .expect("remove canceled waiter");
    store.settle_run(holder, WorkflowStatus::Canceled);
    let before_release = store.ready_nodes().len();
    MutexOps::new(&store)
        .release_run_mutexes(holder)
        .await
        .expect("release holder");

    let immediate = store
        .ready_nodes()
        .get(before_release)
        .cloned()
        .expect("immediate fifo wake");
    assert_eq!(
        immediate.cursor_id,
        node_run(&store, successor, "lock").cursor_id,
        "the canceled oldest waiter is removed and the next active waiter is woken"
    );
    process_ready_node(&store, &immediate)
        .await
        .expect("drive successor");
    assert_eq!(
        node_run(&store, successor, "lock").status,
        WorkflowStatus::Succeeded
    );
}

#[tokio::test]
async fn a_lost_handoff_wake_is_recovered_by_the_existing_poll() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(held_section()));
    let holder = Uuid::now_v7();
    let waiter = Uuid::now_v7();

    start(&store, holder).await;
    start(&store, waiter).await;
    let waiting = node_run(&store, waiter, "lock");
    let poll = store
        .ready_nodes()
        .into_iter()
        .find(|ready| ready.workflow_run_id == waiter && ready.node_id == "lock")
        .expect("periodic mutex poll");

    store.settle_run(holder, WorkflowStatus::Canceled);
    MutexOps::new(&store)
        .release_run_mutexes(holder)
        .await
        .expect("release holder");
    // Deliberately ignore the immediate handoff row, as if its broker drive were lost.
    process_ready_node(&store, &poll)
        .await
        .expect("recovery poll");

    let acquired = node_run(&store, waiter, "lock");
    assert_eq!(acquired.id, waiting.id, "a retry retains its fifo identity");
    assert_eq!(acquired.status, WorkflowStatus::Succeeded);
}

#[tokio::test]
async fn the_owning_cursor_can_reenter_the_same_mutex() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "first" } } },
        {
            "id": "first", "kind": "mutex", "parameters": { "name": "deploy" },
            "transitions": { "on_success": { "$node": "second" } }
        },
        {
            "id": "second", "kind": "mutex", "parameters": { "name": "deploy" },
            "transitions": { "on_success": { "$node": "hold" } }
        },
        {
            "id": "hold", "kind": "signal", "parameters": { "name": "release-me" },
            "transitions": { "on_success": { "$node": "done" } }
        },
        { "id": "done", "kind": "end" }
    ])));
    let run_id = Uuid::now_v7();

    start(&store, run_id).await;

    let first = node_run(&store, run_id, "first");
    let second = node_run(&store, run_id, "second");
    assert_eq!(first.status, WorkflowStatus::Succeeded);
    assert_eq!(second.status, WorkflowStatus::Succeeded);
    assert_eq!(first.cursor_id, second.cursor_id);
}
