//! node-handler behaviour driven directly against an in-memory store.
//!
//! before the reducer's bound was narrowed to `ReducerStore`, these paths were reachable only from
//! `runinator-ws`'s suite, which boots a real sqlite database. that is why the two bugs re-derived
//! below shipped: nothing could reach a parked handler cheaply enough for anyone to write the test.
//!
//! each test here states the production symptom it guards, so a future change that reintroduces the
//! bug fails with an explanation rather than a bare assertion.

use chrono::{Duration, Utc};
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use crate::test_support::FakeStore;
use crate::{ReadyNodeDisposition, process_ready_node};

const WORKFLOW_ID: &str = "11111111-1111-1111-1111-111111111111";
const RUN_ID: &str = "22222222-2222-2222-2222-222222222222";

fn workflow(nodes: serde_json::Value) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "handler test",
        "version": "1.0.0",
        "enabled": true,
        "definition": { "start": "start", "nodes": nodes }
    }))
    .expect("workflow definition")
}

fn queued_run() -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": RUN_ID,
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

fn ready_node(node_id: &str) -> ReadyNodeRecord {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::now_v7(),
        "source_event_id": Uuid::now_v7(),
        "workflow_run_id": RUN_ID,
        "node_id": node_id,
        "status": "queued",
        "ready_at": Utc::now(),
        "attempts": 0,
        "created_at": Utc::now(),
        "updated_at": Utc::now(),
    }))
    .expect("ready node")
}

/// a signal node parks, and once its timeout elapses it leaves the park instead of waiting forever.
///
/// the production bug (fixed 2026-07-06) was that `timed_out` consulted `started_at`, which is only
/// stamped when a node run goes `Running`. a parked node never goes `Running`, so *every* parked kind
/// — signal, approval, input, debounce, mutex, throttle, collect, barrier, await_run, event_source —
/// silently ignored its timeout and parked forever. the fix was `timed_out_since_created`.
///
/// `FakeStore` reproduces that trap faithfully: it only sets `started_at` on `Running`. a handler that
/// goes back to reading `started_at` therefore fails here.
#[tokio::test]
async fn a_parked_signal_node_times_out() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "wait_for_signal" } } },
        {
            "id": "wait_for_signal",
            "kind": "signal",
            "parameters": { "name": "deploy-approved" },
            "timeout_seconds": 60,
            "transitions": {
                "on_success": { "$node": "end" },
                "on_timeout": { "$node": "gave_up" }
            }
        },
        { "id": "gave_up", "kind": "end" },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    // first drive: the node parks.
    let disposition = process_ready_node(&store, &ready_node("wait_for_signal"))
        .await
        .expect("first drive");
    assert_eq!(disposition, ReadyNodeDisposition::Complete);

    let parked = store
        .latest_node_run("wait_for_signal")
        .expect("signal node parked, so it recorded a node run");
    assert_eq!(
        parked.status,
        WorkflowStatus::Waiting,
        "a signal node parks waiting for external delivery"
    );
    assert!(
        parked.started_at.is_none(),
        "a parked node never goes Running, which is exactly why a started_at-based timeout \
         could never fire"
    );

    // age the park past its 60s timeout.
    store.age_node_run(parked.id, Duration::seconds(120));

    let disposition = process_ready_node(&store, &ready_node("wait_for_signal"))
        .await
        .expect("second drive");
    assert_eq!(disposition, ReadyNodeDisposition::Complete);

    let run = store
        .run(parked.workflow_run_id)
        .expect("run still present");
    assert_eq!(
        run.active_node_id.as_deref(),
        Some("gave_up"),
        "an elapsed signal timeout must follow on_timeout; parking forever is the bug this guards"
    );
}

/// a signal node that has not reached its timeout stays parked.
///
/// the mirror of the test above: a timeout that fires early would abandon runs waiting on a
/// legitimately slow external system.
#[tokio::test]
async fn a_signal_node_inside_its_timeout_stays_parked() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "wait_for_signal" } } },
        {
            "id": "wait_for_signal",
            "kind": "signal",
            "parameters": { "name": "deploy-approved" },
            "timeout_seconds": 3600,
            "transitions": {
                "on_success": { "$node": "end" },
                "on_timeout": { "$node": "gave_up" }
            }
        },
        { "id": "gave_up", "kind": "end" },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("wait_for_signal"))
        .await
        .expect("first drive");
    let parked = store.latest_node_run("wait_for_signal").expect("node run");
    store.age_node_run(parked.id, Duration::seconds(60));

    process_ready_node(&store, &ready_node("wait_for_signal"))
        .await
        .expect("second drive");

    let still_parked = store.latest_node_run("wait_for_signal").expect("node run");
    assert_eq!(
        still_parked.status,
        WorkflowStatus::Waiting,
        "60s into a 3600s timeout the node must still be waiting"
    );
    let run = store.run(parked.workflow_run_id).expect("run");
    assert_eq!(
        run.active_node_id.as_deref(),
        Some("wait_for_signal"),
        "the run stays on the parked node until the signal arrives or the timeout elapses"
    );
}

/// a signal delivered out of band resolves the park and follows the success edge.
///
/// this is the happy path the timeout tests bracket: delivery stamps the node run `Succeeded`, and
/// the next drive must move on rather than re-parking.
#[tokio::test]
async fn a_delivered_signal_follows_the_success_edge() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "wait_for_signal" } } },
        {
            "id": "wait_for_signal",
            "kind": "signal",
            "parameters": { "name": "deploy-approved" },
            "transitions": { "on_success": { "$node": "after" } }
        },
        { "id": "after", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("wait_for_signal"))
        .await
        .expect("first drive");
    let parked = store.latest_node_run("wait_for_signal").expect("node run");

    // the delivery endpoint's effect: the parked node run is stamped Succeeded with the payload.
    store.resolve_node_run(
        parked.id,
        WorkflowStatus::Succeeded,
        Some(serde_json::json!({ "approved_by": "ops" }).into()),
    );

    process_ready_node(&store, &ready_node("wait_for_signal"))
        .await
        .expect("second drive");

    let run = store.run(parked.workflow_run_id).expect("run");
    assert_eq!(
        run.active_node_id.as_deref(),
        Some("after"),
        "a delivered signal follows on_success"
    );
}
