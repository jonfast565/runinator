//! stacked, nested, and re-entered control-flow nodes driven through the reducer.
//!
//! these are graph-level cursor tests rather than isolated handler tests: every assertion depends
//! on ready rows being drained across the fan-outs, exactly as the engine drives them in production.

use std::collections::HashSet;

use chrono::Utc;
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use crate::process_ready_node;
use crate::test_support::FakeStore;

const WORKFLOW_ID: &str = "33333333-3333-3333-3333-333333333333";
const RUN_ID: &str = "44444444-4444-4444-4444-444444444444";

fn workflow(nodes: serde_json::Value) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "stacked control flow test",
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

async fn drive(store: &FakeStore) {
    let mut consumed: HashSet<Uuid> = HashSet::new();
    process_ready_node(store, &ready_node("start"))
        .await
        .expect("initial drive");

    for _ in 0..256 {
        let Some(row) = store
            .ready_nodes()
            .into_iter()
            .find(|row| !consumed.contains(&row.id))
        else {
            return;
        };
        consumed.insert(row.id);
        process_ready_node(store, &row).await.expect("drive");
    }
    panic!("the run never stopped arming ready nodes");
}

fn succeeded_runs(store: &FakeStore, node_id: &str) -> usize {
    store
        .node_runs()
        .iter()
        .filter(|run| run.node_id == node_id && run.status == WorkflowStatus::Succeeded)
        .count()
}

fn assert_run_succeeded(store: &FakeStore) {
    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    assert_eq!(run.status, WorkflowStatus::Succeeded, "{:?}", run.message);
}

/// A join may wait for only a subset of a parallel fan-out. The selected branches release the
/// continuation, while an unselected branch remains live on its private terminal path.
#[tokio::test]
async fn selected_join_releases_continuation_while_unselected_branch_is_live() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "fork" } } },
        { "id": "fork", "kind": "parallel",
          "parameters": { "branches": [{ "$node": "lint" }, { "$node": "tests" }, { "$node": "security" }] } },
        { "id": "lint", "kind": "audit", "parameters": { "action": "lint" },
          "transitions": { "on_success": { "$node": "join" } } },
        { "id": "tests", "kind": "audit", "parameters": { "action": "tests" },
          "transitions": { "on_success": { "$node": "join" } } },
        { "id": "security", "kind": "action",
          "action": { "provider": "test", "function": "wait", "configuration": {} },
          "transitions": { "on_success": { "$node": "security_end" } } },
        { "id": "join", "kind": "join",
          "parameters": { "wait_for": [{ "$node": "lint" }, { "$node": "tests" }], "mode": "all" },
          "transitions": { "on_success": { "$node": "after" } } },
        { "id": "after", "kind": "audit", "parameters": { "action": "after" },
          "transitions": { "on_success": { "$node": "end" } } },
        { "id": "security_end", "kind": "end" },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("fan out");
    process_ready_node(&store, &ready_node("lint"))
        .await
        .expect("complete lint");
    process_ready_node(&store, &ready_node("tests"))
        .await
        .expect("complete tests and join");

    assert_eq!(
        succeeded_runs(&store, "after"),
        1,
        "selected join should release after"
    );
    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    assert_eq!(
        run.status,
        WorkflowStatus::Running,
        "security is still live: {:?}",
        run.execution_state
    );
    assert!(
        run.execution_state
            .cursors
            .iter()
            .any(|cursor| cursor.is_at("security")),
        "the unselected branch must not traverse the post-join continuation"
    );
}

/// two fan-out/reconvergence pairs are stacked inside a `for` body and revisited three times.
///
/// this simultaneously pins branch retirement, one-cursor join continuation, and freshness of
/// both parallel and join node runs on every back-edge visit.
#[tokio::test]
async fn stacked_parallel_joins_repeat_for_every_for_each_item() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each", "kind": "loop", "parameters": { "items": [1, 2, 3] },
            "transitions": { "next": { "$node": "fan_one" }, "on_success": { "$node": "end" } }
        },
        {
            "id": "fan_one", "kind": "parallel",
            "parameters": { "branches": [{ "$node": "one_left" }, { "$node": "one_right" }] }
        },
        { "id": "one_left", "kind": "output", "transitions": { "on_success": { "$node": "join_one" } } },
        { "id": "one_right", "kind": "output", "transitions": { "on_success": { "$node": "join_one" } } },
        {
            "id": "join_one", "kind": "join",
            "parameters": { "wait_for": [{ "$node": "one_left" }, { "$node": "one_right" }], "mode": "all" },
            "transitions": { "on_success": { "$node": "fan_two" } }
        },
        {
            "id": "fan_two", "kind": "parallel",
            "parameters": { "branches": [{ "$node": "two_left" }, { "$node": "two_right" }] }
        },
        { "id": "two_left", "kind": "output", "transitions": { "on_success": { "$node": "join_two" } } },
        { "id": "two_right", "kind": "output", "transitions": { "on_success": { "$node": "join_two" } } },
        {
            "id": "join_two", "kind": "join",
            "parameters": { "wait_for": [{ "$node": "two_left" }, { "$node": "two_right" }], "mode": "all" },
            "transitions": { "on_success": { "$node": "each" } }
        },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    for node_id in [
        "fan_one",
        "one_left",
        "one_right",
        "join_one",
        "fan_two",
        "two_left",
        "two_right",
        "join_two",
    ] {
        assert_eq!(
            succeeded_runs(&store, node_id),
            3,
            "{node_id} must run exactly once per outer item"
        );
    }
    assert_run_succeeded(&store);
}

/// an inner fan-out reconverges into one cursor before that cursor joins an outer sibling.
///
/// the outer join waits on the inner join node, not either inner leaf; this proves the nested
/// region is treated as one completed outer branch and does not strand or leak an inner cursor.
#[tokio::test]
async fn nested_parallel_regions_reconverge_from_the_inside_out() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "outer_fan" } } },
        {
            "id": "outer_fan", "kind": "parallel",
            "parameters": { "branches": [{ "$node": "inner_fan" }, { "$node": "outer_side" }] }
        },
        {
            "id": "inner_fan", "kind": "parallel",
            "parameters": { "branches": [{ "$node": "inner_left" }, { "$node": "inner_right" }] }
        },
        { "id": "inner_left", "kind": "output", "transitions": { "on_success": { "$node": "inner_join" } } },
        { "id": "inner_right", "kind": "output", "transitions": { "on_success": { "$node": "inner_join" } } },
        {
            "id": "inner_join", "kind": "join",
            "parameters": { "wait_for": [{ "$node": "inner_left" }, { "$node": "inner_right" }], "mode": "all" },
            "transitions": { "on_success": { "$node": "outer_join" } }
        },
        { "id": "outer_side", "kind": "output", "transitions": { "on_success": { "$node": "outer_join" } } },
        {
            "id": "outer_join", "kind": "join",
            "parameters": { "wait_for": [{ "$node": "inner_join" }, { "$node": "outer_side" }], "mode": "all" },
            "transitions": { "on_success": { "$node": "end" } }
        },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    for node_id in [
        "outer_fan",
        "inner_fan",
        "inner_left",
        "inner_right",
        "inner_join",
        "outer_side",
        "outer_join",
    ] {
        assert_eq!(succeeded_runs(&store, node_id), 1, "{node_id}");
    }
    assert_run_succeeded(&store);
}

/// a race is itself one branch of a parallel region and must reconverge with the other branch.
///
/// the race replaces its incoming outer cursor with contender cursors. its winner must still be
/// able to satisfy the outer join after the losing contender is retired.
#[tokio::test]
async fn a_race_nested_in_parallel_reconverges_at_the_outer_join() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "outer_fan" } } },
        {
            "id": "outer_fan", "kind": "parallel",
            "parameters": { "branches": [{ "$node": "race" }, { "$node": "outer_side" }] }
        },
        {
            "id": "race", "kind": "race",
            "parameters": { "branches": [{ "$node": "fast" }, { "$node": "slow" }], "winner": "first_success" },
            "transitions": { "on_success": { "$node": "outer_join" } }
        },
        { "id": "fast", "kind": "output", "transitions": { "on_success": { "$node": "race" } } },
        { "id": "slow", "kind": "output", "transitions": { "on_success": { "$node": "race" } } },
        { "id": "outer_side", "kind": "output", "transitions": { "on_success": { "$node": "outer_join" } } },
        {
            "id": "outer_join", "kind": "join",
            "parameters": { "wait_for": [{ "$node": "race" }, { "$node": "outer_side" }], "mode": "all" },
            "transitions": { "on_success": { "$node": "end" } }
        },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    assert_eq!(succeeded_runs(&store, "race"), 1);
    assert_eq!(
        succeeded_runs(&store, "fast") + succeeded_runs(&store, "slow"),
        1
    );
    assert_eq!(succeeded_runs(&store, "outer_join"), 1);
    assert_run_succeeded(&store);
}

/// the same race node is entered once per `for` item.
///
/// prior contender results and the prior terminal race run must not satisfy the next visit. every
/// visit creates fresh contenders, elects one winner, and returns the surviving cursor to the loop.
#[tokio::test]
async fn a_race_is_reentrant_across_for_each_iterations() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each", "kind": "loop", "parameters": { "items": ["a", "b", "c"] },
            "transitions": { "next": { "$node": "race" }, "on_success": { "$node": "end" } }
        },
        {
            "id": "race", "kind": "race",
            "parameters": { "branches": [{ "$node": "fast" }, { "$node": "slow" }], "winner": "first_success" },
            "transitions": { "on_success": { "$node": "each" } }
        },
        { "id": "fast", "kind": "output", "transitions": { "on_success": { "$node": "race" } } },
        { "id": "slow", "kind": "output", "transitions": { "on_success": { "$node": "race" } } },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    assert_eq!(
        succeeded_runs(&store, "race"),
        3,
        "one race must settle per item; node runs: {:?}",
        store
            .node_runs()
            .iter()
            .map(|run| (&run.node_id, run.status, &run.transition_reason))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        succeeded_runs(&store, "fast") + succeeded_runs(&store, "slow"),
        3,
        "old contender results must not win a later visit"
    );
    assert_run_succeeded(&store);
}

/// an `all` race waits for every contender and remains fresh across a loop back-edge.
///
/// the first successful contender cannot decide this policy, so its cursor retires while the
/// sibling continues. the final contender carries the run onward, once per outer item.
#[tokio::test]
async fn an_all_policy_race_waits_for_every_contender_on_every_visit() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each", "kind": "loop", "parameters": { "items": [1, 2] },
            "transitions": { "next": { "$node": "race" }, "on_success": { "$node": "end" } }
        },
        {
            "id": "race", "kind": "race",
            "parameters": { "branches": [{ "$node": "left" }, { "$node": "right" }], "winner": "all" },
            "transitions": { "on_success": { "$node": "each" } }
        },
        { "id": "left", "kind": "output", "transitions": { "on_success": { "$node": "race" } } },
        { "id": "right", "kind": "output", "transitions": { "on_success": { "$node": "race" } } },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    assert_eq!(succeeded_runs(&store, "left"), 2);
    assert_eq!(succeeded_runs(&store, "right"), 2);
    assert_eq!(succeeded_runs(&store, "race"), 2);
    assert_run_succeeded(&store);
}

/// a losing ready row may land after its cursor was retired and the next loop lap has fanned out.
///
/// the stale row is still addressed to the old cursor. it must be discarded rather than falling
/// back to the new lap's contender at the same node and manufacturing a winner for that lap.
#[tokio::test]
async fn a_late_race_loser_does_not_drive_the_next_laps_contender() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each", "kind": "loop", "parameters": { "items": [1, 2] },
            "transitions": { "next": { "$node": "race" }, "on_success": { "$node": "end" } }
        },
        {
            "id": "race", "kind": "race",
            "parameters": { "branches": [{ "$node": "fast" }, { "$node": "slow" }], "winner": "first_success" },
            "transitions": { "on_success": { "$node": "each" } }
        },
        { "id": "fast", "kind": "output", "transitions": { "on_success": { "$node": "race" } } },
        { "id": "slow", "kind": "output", "transitions": { "on_success": { "$node": "race" } } },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("initial drive");
    let first_lap = store.ready_nodes();
    let first_fast = first_lap
        .iter()
        .find(|row| row.node_id == "fast")
        .expect("first fast contender")
        .clone();
    let late_slow = first_lap
        .iter()
        .find(|row| row.node_id == "slow")
        .expect("first slow contender")
        .clone();

    process_ready_node(&store, &first_fast)
        .await
        .expect("first lap winner");
    assert_eq!(succeeded_runs(&store, "race"), 1);
    assert_eq!(succeeded_runs(&store, "fast"), 1);
    let late_cursor = late_slow.cursor_id.expect("addressed loser");
    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    assert!(
        run.execution_state.cursor(late_cursor).is_none(),
        "the losing cursor must retire when the first lap settles"
    );

    process_ready_node(&store, &late_slow)
        .await
        .expect("late first-lap loser");
    assert_eq!(
        succeeded_runs(&store, "race"),
        1,
        "the stale loser must not settle the second lap"
    );
    assert_eq!(
        succeeded_runs(&store, "slow"),
        0,
        "the stale row must not be rebound to the second lap's slow cursor"
    );

    let second_fast = store
        .ready_nodes()
        .into_iter()
        .find(|row| row.node_id == "fast" && row.id != first_fast.id)
        .expect("second fast contender");
    process_ready_node(&store, &second_fast)
        .await
        .expect("second lap winner");

    assert_eq!(succeeded_runs(&store, "race"), 2);
    assert_eq!(succeeded_runs(&store, "fast"), 2);
    assert_run_succeeded(&store);
}
