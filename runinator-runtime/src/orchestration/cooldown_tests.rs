//! the cooldown gate: a named, cross-run window that skips a body if it ran too recently.
//!
//! it was unreachable from a unit test until `FakeStore` learned automation records, which is how a
//! terminal-status bug lived in it. the short-circuit is the interesting half: it ends a thread of
//! control without executing anything, so getting the settle wrong is invisible on a linear run and
//! wrong on every forked one.

use chrono::Utc;
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use crate::process_ready_node;
use crate::test_support::FakeStore;

const WORKFLOW_ID: &str = "11111111-1111-1111-1111-111111111111";

fn workflow(nodes: serde_json::Value) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "cooldown test",
        "version": "1.0.0",
        "enabled": true,
        "definition": { "start": "start", "nodes": nodes }
    }))
    .expect("workflow definition")
}

fn run(run_id: Uuid, state: serde_json::Value) -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": run_id,
        "workflow_id": WORKFLOW_ID,
        "status": "queued",
        "active_node_id": null,
        "parameters": {},
        "state": state,
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

/// start -> gate (cooldown) -> body -> done.
fn gated(window_seconds: i64) -> serde_json::Value {
    serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "gate" } } },
        { "id": "gate", "kind": "cooldown",
          "parameters": { "name": "nightly", "window_seconds": window_seconds },
          "transitions": { "on_success": { "$node": "body" } } },
        { "id": "body", "kind": "audit", "parameters": { "action": "ran" },
          "transitions": { "on_success": { "$node": "done" } } },
        { "id": "done", "kind": "end" }
    ])
}

fn window(store: &FakeStore) -> Option<i64> {
    store.cooldown_window("nightly")
}

// the first pass through an unstamped gate runs the body and opens the window.
#[tokio::test]
async fn a_cold_gate_runs_the_body_and_stamps_the_window() {
    let store = FakeStore::new();
    let run_id = Uuid::now_v7();
    store.insert_workflow(workflow(gated(3600)));
    store.insert_run(run(run_id, serde_json::json!({})));

    process_ready_node(&store, &ready_node(run_id, "start"))
        .await
        .expect("drive");

    assert!(
        store.latest_node_run("body").is_some(),
        "a cold gate must not skip the body"
    );
    assert!(window(&store).is_some(), "the pass opens the window");
    assert_eq!(
        store.run(run_id).expect("run").status,
        WorkflowStatus::Succeeded
    );
}

// a second run inside the window skips the body. this is the gate actually gating, and it needs two
// runs because the window is deliberately cross-run.
#[tokio::test]
async fn a_second_run_inside_the_window_skips_the_body() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(gated(3600)));

    let first = Uuid::now_v7();
    store.insert_run(run(first, serde_json::json!({})));
    process_ready_node(&store, &ready_node(first, "start"))
        .await
        .expect("first run");
    let stamped = window(&store).expect("first pass stamped the window");

    let second = Uuid::now_v7();
    store.insert_run(run(second, serde_json::json!({})));
    process_ready_node(&store, &ready_node(second, "start"))
        .await
        .expect("second run");

    let gate_runs: Vec<_> = store
        .node_runs()
        .into_iter()
        .filter(|node_run| node_run.workflow_run_id == second && node_run.node_id == "gate")
        .collect();
    assert_eq!(gate_runs.len(), 1);
    assert_eq!(
        gate_runs[0].transition_reason.as_deref(),
        Some("cooldown_skipped"),
        "the second run is inside the window, so its gate skips"
    );
    assert!(
        !store
            .node_runs()
            .iter()
            .any(|node_run| node_run.workflow_run_id == second && node_run.node_id == "body"),
        "a skipped gate must not execute the body"
    );
    assert_eq!(
        window(&store),
        Some(stamped),
        "skipping must not extend the window, or a busy schedule could hold the gate shut forever"
    );
}

// the window has to be re-stampable. `stamp_cooldown` updates the existing record by its id, and
// silently did nothing when it could not read one back -- which would leave the gate permanently
// open after its first window elapsed.
#[tokio::test]
async fn an_elapsed_window_is_re_stamped_rather_than_left_open() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(gated(3600)));

    // a window stamped long ago: elapsed, so this pass runs the body *and* must move the stamp
    // forward. seeding it stale rather than driving a first run is what makes the assertion sharp
    // -- two passes in the same second would both satisfy a `>=` even if the update never happened.
    let stale = Utc::now().timestamp() - 10_000;
    store.seed_cooldown("nightly", stale);

    let run_id = Uuid::now_v7();
    store.insert_run(run(run_id, serde_json::json!({})));
    process_ready_node(&store, &ready_node(run_id, "start"))
        .await
        .expect("drive");

    assert!(
        store.latest_node_run("body").is_some(),
        "an elapsed window runs the body"
    );
    assert!(
        window(&store).expect("the record still exists") > stale,
        "the stamp must move forward; leaving it stale means every later run finds the window \
         elapsed and the gate never closes again"
    );
}

// the bug this file exists for: a skipped gate used to write `Succeeded` onto the *run*. on a forked
// run that ends everything while sibling branches are still executing.
#[tokio::test]
async fn a_skipped_gate_ends_only_its_own_branch() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "fork" } } },
        { "id": "fork", "kind": "parallel",
          "parameters": { "branches": [{ "$node": "gate" }, { "$node": "other" }] } },
        // this branch is inside the window and skips.
        { "id": "gate", "kind": "cooldown",
          "parameters": { "name": "nightly", "window_seconds": 3600 } },
        // this one is a worker-bound action, so it stays live while the gate settles.
        { "id": "other", "kind": "action",
          "action": { "provider": "test", "function": "work", "configuration": {} },
          "transitions": { "on_success": { "$node": "done" } } },
        { "id": "done", "kind": "end" }
    ])));

    // pre-stamp the window so the gate branch short-circuits on its first visit.
    store.seed_cooldown("nightly", Utc::now().timestamp());

    let run_id = Uuid::now_v7();
    store.insert_run(run(run_id, serde_json::json!({})));
    process_ready_node(&store, &ready_node(run_id, "start"))
        .await
        .expect("fan out");
    process_ready_node(&store, &ready_node(run_id, "gate"))
        .await
        .expect("drive the gated branch");

    let settled = store.run(run_id).expect("run");
    assert_ne!(
        settled.status,
        WorkflowStatus::Succeeded,
        "one branch skipping its cooldown must not finish the run under a live sibling"
    );
    let state = settled.execution_state;
    assert!(
        state.cursors.iter().any(|cursor| cursor.is_at("other")),
        "the sibling branch keeps its thread of control, got {:?}",
        state
            .cursors
            .iter()
            .map(|cursor| cursor.node_id().to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        !state.cursors.iter().any(|cursor| cursor.is_at("gate")),
        "and the skipped branch retired itself"
    );
}

// the race the atomic claim exists for: two runs reaching one gate at the same moment.
//
// read-decide-write let both observe an elapsed window and both enter the body, which is the single
// thing a gate must prevent. `FakeStore` decides and stamps under one lock with no await between,
// so it stands in for the sql statement doing both at once -- a fake that read and then wrote would
// let this pass against a racy backend.
#[tokio::test]
async fn concurrent_runs_cannot_both_claim_one_window() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(gated(3600)));

    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    store.insert_run(run(first, serde_json::json!({})));
    store.insert_run(run(second, serde_json::json!({})));

    let first_ready = ready_node(first, "start");
    let second_ready = ready_node(second, "start");
    let (left, right) = tokio::join!(
        process_ready_node(&store, &first_ready),
        process_ready_node(&store, &second_ready),
    );
    left.expect("first run");
    right.expect("second run");

    let bodies = store
        .node_runs()
        .into_iter()
        .filter(|node_run| node_run.node_id == "body")
        .count();
    assert_eq!(
        bodies, 1,
        "exactly one of two concurrent runs may pass the gate; {bodies} entered the body"
    );

    let skipped = store
        .node_runs()
        .into_iter()
        .filter(|node_run| {
            node_run.node_id == "gate"
                && node_run.transition_reason.as_deref() == Some("cooldown_skipped")
        })
        .count();
    assert_eq!(skipped, 1, "and the loser skips rather than failing");
}

// a window whose seconds are zero or negative is always claimable, and must not panic or wrap.
#[tokio::test]
async fn a_zero_window_admits_every_pass() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(gated(0)));

    for _ in 0..2 {
        let run_id = Uuid::now_v7();
        store.insert_run(run(run_id, serde_json::json!({})));
        process_ready_node(&store, &ready_node(run_id, "start"))
            .await
            .expect("drive");
    }

    assert_eq!(
        store
            .node_runs()
            .into_iter()
            .filter(|node_run| node_run.node_id == "body")
            .count(),
        2,
        "a zero-length window never holds anyone back"
    );
}
