//! the `loop` node driven against the in-memory store: nesting, the try interaction, what `last`
//! is scoped to, and the two missing-edge cases.
//!
//! every test here states the production symptom it guards. the whole cluster traces back to one
//! decision — deriving the iteration index by counting the loop node's succeeded runs instead of
//! storing it — so a change that reintroduces that derivation fails here with an explanation.

use std::collections::HashSet;

use chrono::Utc;
use runinator_models::cursor::RunCursor;
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::value::Value;
use runinator_models::workflow_state::TryFrame;
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus,
};
use uuid::Uuid;

use crate::process_ready_node;
use crate::test_support::FakeStore;

const WORKFLOW_ID: &str = "11111111-1111-1111-1111-111111111111";
const RUN_ID: &str = "22222222-2222-2222-2222-222222222222";

fn workflow(nodes: serde_json::Value) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "control flow test",
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

fn ready_node_for_cursor(node_id: &str, cursor_id: Uuid) -> ReadyNodeRecord {
    let mut ready = ready_node(node_id);
    ready.cursor_id = Some(cursor_id);
    ready
}

fn node_run(
    node_id: &str,
    cursor_id: Uuid,
    status: WorkflowStatus,
    output: Option<Value>,
) -> WorkflowNodeRun {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::now_v7(),
        "workflow_run_id": RUN_ID,
        "node_id": node_id,
        "cursor_id": cursor_id,
        "status": status,
        "attempt": 1,
        "parameters": {},
        "output_json": output,
        "state": null,
        "transition_reason": null,
        "created_at": Utc::now(),
        "started_at": Utc::now(),
        "finished_at": status.is_terminal().then(Utc::now),
        "message": null,
    }))
    .expect("workflow node run")
}

fn parallel_try_workflow() -> WorkflowDefinition {
    workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "fan" } } },
        {
            "id": "fan", "kind": "parallel",
            "parameters": { "branches": [{ "$node": "guard" }, { "$node": "sibling" }] }
        },
        {
            "id": "guard", "kind": "try",
            "parameters": {
                "body": { "$node": "body" },
                "catch": { "$node": "catch" }
            },
            "transitions": { "on_success": { "$node": "join" } }
        },
        { "id": "body", "kind": "output", "transitions": { "on_success": { "$node": "guard" } } },
        { "id": "catch", "kind": "output", "transitions": { "on_success": { "$node": "guard" } } },
        { "id": "sibling", "kind": "output", "transitions": { "on_success": { "$node": "join" } } },
        {
            "id": "join", "kind": "join",
            "parameters": {
                "wait_for": [{ "$node": "guard" }, { "$node": "sibling" }],
                "mode": "all"
            },
            "transitions": { "on_success": { "$node": "end" } }
        },
        { "id": "end", "kind": "end" }
    ]))
}

async fn assert_parallel_try_output(
    phase: &str,
    phase_node: &str,
    own_output: Value,
    sibling_output: Value,
    seed_prior_visit: bool,
) {
    let store = FakeStore::new();
    store.insert_workflow(parallel_try_workflow());

    let mut branch = RunCursor::forked("guard", "fan");
    branch.try_frame = Some(TryFrame {
        node_id: "guard".into(),
        phase: phase.into(),
        pending_status: None,
        pending_output: None,
    });
    let sibling = RunCursor::forked("sibling", "fan");
    let branch_id = branch.id;
    let sibling_id = sibling.id;

    let mut run = queued_run();
    run.status = WorkflowStatus::Running;
    run.active_node_id = Some("guard".into());
    run.state = serde_json::to_value(serde_json::json!({
        "cursors": [branch, sibling]
    }))
    .expect("run state")
    .into();
    store.insert_run(run);

    if seed_prior_visit {
        store.insert_node_run(node_run(
            "guard",
            branch_id,
            WorkflowStatus::Succeeded,
            Some(serde_json::json!({ "value": "prior try" }).into()),
        ));
        store.insert_node_run(node_run(
            phase_node,
            branch_id,
            WorkflowStatus::Succeeded,
            Some(serde_json::json!({ "value": "prior visit" }).into()),
        ));
    }

    store.insert_node_run(node_run("guard", branch_id, WorkflowStatus::Running, None));
    store.insert_node_run(node_run(
        phase_node,
        branch_id,
        WorkflowStatus::Succeeded,
        Some(own_output.clone()),
    ));
    // this is deliberately newer than the active branch's phase result. a run-wide reverse scan
    // selects it even though it belongs to the other parallel cursor.
    store.insert_node_run(node_run(
        "sibling",
        sibling_id,
        WorkflowStatus::Succeeded,
        Some(sibling_output),
    ));

    process_ready_node(&store, &ready_node_for_cursor("guard", branch_id))
        .await
        .expect("try branch drive");

    let settled = store
        .node_runs()
        .into_iter()
        .filter(|run| run.node_id == "guard" && run.cursor_id == Some(branch_id))
        .max_by_key(|run| run.id)
        .expect("settled try run");
    assert_eq!(settled.status, WorkflowStatus::Succeeded);
    assert_eq!(settled.output_json, Some(own_output));
}

/// drive the run from `start`, then drain every ready row the run arms, until none is left.
///
/// one `process_ready_node` call follows a cursor for up to 64 inline steps, but a fan-out ends the
/// drive and arms a row per branch, so the graphs here need the drain. `FakeStore` never marks a row
/// consumed, so rows are tracked by id — scanning for a `Queued` row instead re-picks the first one
/// forever and the run looks wedged when it is only the harness standing still.
async fn drive(store: &FakeStore) {
    let mut consumed: HashSet<Uuid> = HashSet::new();
    process_ready_node(store, &ready_node("start"))
        .await
        .expect("initial drive");

    for _ in 0..64 {
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

/// a try controller resumes after its body, so another parallel branch can record a newer output
/// before the controller is driven. the try result still belongs to the controller's own cursor.
#[tokio::test]
async fn try_body_output_does_not_leak_across_parallel_branches() {
    assert_parallel_try_output(
        "body",
        "body",
        serde_json::json!({ "value": "body branch" }).into(),
        serde_json::json!({ "value": "sibling branch" }).into(),
        false,
    )
    .await;
}

/// catch output has the same cursor boundary as body output. seeding a completed earlier visit also
/// pins the lower bound: re-entry must use this visit's catch, not an older result from this cursor.
#[tokio::test]
async fn reentered_try_catch_output_stays_in_its_parallel_branch_and_visit() {
    assert_parallel_try_output(
        "catch",
        "catch",
        serde_json::json!({ "value": "current catch" }).into(),
        serde_json::json!({ "value": "sibling branch" }).into(),
        true,
    )
    .await;
}

/// the headline bug. `ctx.node_runs` is the whole run history, so counting the inner loop's
/// succeeded runs made it count the *previous outer lap's* runs as its own: on the second outer
/// pass its derived index was already past its item count and it exhausted without running its
/// body once. two items each therefore produced two leaf runs instead of four.
///
/// nothing nested a `for` inside a `for` in the test suite, and the one place the codebase does it
/// (`packs/sdlc/rexrap/sdlc-deploy.rexrap`) puts the inner loop inside a `parallel`, which forks a fresh
/// cursor per outer lap and hid the bug.
#[tokio::test]
async fn a_nested_loop_runs_its_body_on_every_outer_lap() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "outer" } } },
        {
            "id": "outer",
            "kind": "loop",
            "parameters": { "items": ["a", "b"] },
            "transitions": { "next": { "$node": "inner" }, "on_success": { "$node": "end" } }
        },
        {
            "id": "inner",
            "kind": "loop",
            "parameters": { "items": [1, 2] },
            // exhausting the inner loop hands control back to the outer one for its next lap.
            "transitions": { "next": { "$node": "leaf" }, "on_success": { "$node": "outer" } }
        },
        { "id": "leaf", "kind": "output", "transitions": { "on_success": { "$node": "inner" } } },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    assert_eq!(
        succeeded_runs(&store, "leaf"),
        4,
        "two outer laps of two inner items each; a derived index gives 2, because the inner loop \
         counts the first outer lap's runs against its own item count"
    );
    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    assert_eq!(run.status, WorkflowStatus::Succeeded);
}

/// the loop handler used to call `cursor.clear_frames()` on every lap, which nulled `try_frame`
/// as well.
///
/// the loop is in the `finally` region deliberately: `TryOp` defaults a missing frame to
/// `phase: "body"`, so a loop in the *body* hides the bug — resetting to `body` while the body is
/// what is running is a no-op. From `finally` the reset is visible, because the phase machine falls
/// back to `body`, sees the body already succeeded, and starts `finally` over. The frame also
/// carries `pending_status`/`pending_output`, which the reset discards.
#[tokio::test]
async fn a_loop_inside_a_try_region_leaves_the_try_phase_alone() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "guard" } } },
        {
            "id": "guard",
            "kind": "try",
            "parameters": { "body": { "$node": "work" }, "finally": { "$node": "each" } },
            "transitions": { "on_success": { "$node": "end" } }
        },
        { "id": "work", "kind": "output", "transitions": { "on_success": { "$node": "guard" } } },
        {
            "id": "each",
            "kind": "loop",
            "parameters": { "items": ["x", "y"] },
            "transitions": { "next": { "$node": "leaf" }, "on_success": { "$node": "guard" } }
        },
        { "id": "leaf", "kind": "output", "transitions": { "on_success": { "$node": "each" } } },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    assert_eq!(succeeded_runs(&store, "leaf"), 2, "both items run once");
    assert_eq!(
        succeeded_runs(&store, "work"),
        1,
        "the body must not re-run; a wiped try frame drops the phase back to `body` and the try \
         restarts its finally region without end"
    );
    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    assert_eq!(run.status, WorkflowStatus::Succeeded);
}

/// `last` used to come from a run-wide reverse scan for the newest succeeded run that was not the
/// loop itself, so under fan-out it returned whatever branch happened to finish last rather than
/// this loop's own previous iteration.
#[tokio::test]
async fn loop_last_reports_this_loops_previous_iteration() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each",
            "kind": "loop",
            "parameters": { "items": ["first", "second"] },
            "transitions": { "next": { "$node": "leaf" }, "on_success": { "$node": "end" } }
        },
        {
            "id": "leaf",
            "kind": "output",
            "parameters": { "value": { "$ref": { "node": "each", "output": ["item"] } } },
            "transitions": { "on_success": { "$node": "each" } }
        },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    let laps: Vec<Value> = store
        .node_runs()
        .iter()
        .filter(|run| run.node_id == "each" && run.status == WorkflowStatus::Succeeded)
        .filter_map(|run| run.output_json.clone())
        .collect();
    assert_eq!(laps.len(), 3, "two iterations plus the exhausting visit");
    assert!(
        laps[0].get("last").is_none(),
        "the first visit has no previous iteration"
    );
    for lap in &laps[1..] {
        assert!(
            lap.get("last").is_some(),
            "every visit after the first carries the previous iteration's body output"
        );
    }
    let results = laps[2]
        .get("results")
        .and_then(Value::as_array)
        .expect("accumulated results");
    assert_eq!(results.len(), 2);
    assert_eq!(results.first(), laps[1].get("last"));
    assert_eq!(results.get(1), laps[2].get("last"));
}

/// a loop whose body edge is missing used to target itself, spinning against the engine's inline
/// step limit and reporting a blocked run with nothing to explain it.
#[tokio::test]
async fn a_loop_without_a_body_edge_blocks_with_a_reason() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each",
            "kind": "loop",
            "parameters": { "items": ["a"] },
            "transitions": { "on_success": { "$node": "end" } }
        },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    assert_eq!(run.status, WorkflowStatus::Blocked);
    assert!(
        run.message
            .as_deref()
            .is_some_and(|message| message.contains("body target")),
        "the block must name the missing edge, got {:?}",
        run.message
    );
}

/// exhaustion goes out by `on_success`. it must not fall through `next_transition`'s success
/// fallback to `transitions.next`, because for a loop that is the *body* — a loop authored without
/// an exit edge would otherwise re-enter its body forever.
#[tokio::test]
async fn an_exhausted_loop_without_an_exit_edge_retires_instead_of_re_entering_its_body() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each",
            "kind": "loop",
            "parameters": { "items": ["a", "b"] },
            "transitions": { "next": { "$node": "leaf" } }
        },
        { "id": "leaf", "kind": "output", "transitions": { "on_success": { "$node": "each" } } },
        // present only to satisfy the graph validator; deliberately not the loop's exit edge.
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    assert_eq!(
        succeeded_runs(&store, "leaf"),
        2,
        "each item runs exactly once; falling back to `next` reruns the body without end"
    );
}

/// the runtime used to read a missing or non-array `items` as an empty list, so a loop over an
/// upstream value that came back null produced a workflow that succeeded having done nothing.
#[tokio::test]
async fn a_loop_whose_items_are_not_an_array_blocks() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each",
            "kind": "loop",
            "parameters": { "items": { "$ref": { "input": ["absent"] } } },
            "transitions": { "next": { "$node": "leaf" }, "on_success": { "$node": "end" } }
        },
        { "id": "leaf", "kind": "output", "transitions": { "on_success": { "$node": "each" } } },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    assert_eq!(
        run.status,
        WorkflowStatus::Blocked,
        "silently iterating nothing is the failure nobody can see"
    );
    assert_eq!(succeeded_runs(&store, "leaf"), 0);
}

/// a fan-out inside a loop body must come back knowing which lap it is on.
///
/// `RunCursor::forked` inherited no frames, and the forking cursor retires, so a `parallel` in a
/// loop body threw the loop position away: whichever branch cursor survived the join re-entered the
/// loop at index 0 and the loop never terminated. `speculative_from` had always cloned frames for
/// exactly this reason — a fork explores from where its parent stands.
///
/// `packs/sdlc/rexrap/sdlc-deploy.rexrap` nests its inner `for` inside a `parallel` branch, so this is
/// the shape real packs use.
#[tokio::test]
async fn a_parallel_inside_a_loop_body_keeps_the_loop_position() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
        {
            "id": "each",
            "kind": "loop",
            "parameters": { "items": ["a", "b", "c"] },
            "transitions": { "next": { "$node": "fan" }, "on_success": { "$node": "end" } }
        },
        {
            "id": "fan",
            "kind": "parallel",
            "parameters": { "branches": [{ "$node": "left" }, { "$node": "right" }] },
            "transitions": {}
        },
        { "id": "left", "kind": "output", "transitions": { "on_success": { "$node": "join" } } },
        { "id": "right", "kind": "output", "transitions": { "on_success": { "$node": "join" } } },
        {
            "id": "join",
            "kind": "join",
            "parameters": { "wait_for": [{ "$node": "left" }, { "$node": "right" }], "mode": "all" },
            "transitions": { "on_success": { "$node": "each" } }
        },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    drive(&store).await;

    // three laps, not two: the join bounds "which branches am I joining" by its own last settled
    // run, so a third lap is what proves that bound keeps advancing instead of collapsing onto one
    // recycled row and letting lap two's branches satisfy lap three.
    assert_eq!(succeeded_runs(&store, "left"), 3, "one fan-out per item");
    assert_eq!(
        succeeded_runs(&store, "right"),
        3,
        "both branches run every lap"
    );
    assert_eq!(
        succeeded_runs(&store, "join"),
        3,
        "the join settles once per lap; a join satisfied by the previous lap's branches lets every \
         branch through unjoined and settles fewer times than there were laps"
    );
    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    assert_eq!(run.status, WorkflowStatus::Succeeded);
}
