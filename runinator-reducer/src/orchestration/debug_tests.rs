//! the debugger's execution half, driven against the in-memory store.
//!
//! none of this was reachable before: `Debuggable::should_break_at` had no callers, nothing ever
//! wrote `DebugPaused`, and the debug endpoints enqueued nothing, so a "paused" run just kept
//! running. every test here states the behaviour that made the debugger inert, so a regression
//! fails with an explanation rather than a bare assertion.

use chrono::Utc;
use runinator_models::cursor::RunCursor;
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::value::Value;
use runinator_models::workflow_state::{DebugRuntime, WorkflowRunState};
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use crate::process_ready_node;
use crate::test_support::FakeStore;

const WORKFLOW_ID: &str = "11111111-1111-1111-1111-111111111111";
const RUN_ID: &str = "22222222-2222-2222-2222-222222222222";

fn workflow(nodes: serde_json::Value) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "debug test",
        "version": "1.0.0",
        "enabled": true,
        "definition": { "start": "start", "nodes": nodes }
    }))
    .expect("workflow definition")
}

/// a run seeded with a debug frame, as `create_workflow_run(debug: true)` does.
fn debug_run(mode: &str) -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": RUN_ID,
        "workflow_id": WORKFLOW_ID,
        "status": "queued",
        "active_node_id": null,
        "parameters": {},
        "state": { "debug": { "enabled": true, "mode": mode, "breakpoints": [] } },
        "created_at": Utc::now(),
        "started_at": null,
        "finished_at": null,
        "message": null,
    }))
    .expect("workflow run")
}

fn run_with_state(state: serde_json::Value) -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": RUN_ID,
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

/// a three-node line: start -> middle -> done.
fn linear() -> serde_json::Value {
    serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "middle" } } },
        { "id": "middle", "kind": "audit",
          "parameters": { "action": "noted" },
          "transitions": { "on_success": { "$node": "done" } } },
        { "id": "done", "kind": "end" }
    ])
}

fn state_of(store: &FakeStore) -> WorkflowRunState {
    WorkflowRunState::from_state(&store.run(RUN_ID.parse().unwrap()).expect("run").state)
}

/// grant a step to whichever cursor is parked, the way `step_debug_cursor` does.
fn request_step(store: &FakeStore) {
    let run_id: Uuid = RUN_ID.parse().unwrap();
    let mut run = store.run(run_id).expect("run");
    let mut state = WorkflowRunState::from_state(&run.state);
    let target = state
        .cursors
        .iter()
        .find(|cursor| state.cursor_debug(cursor.id).paused)
        .map(|cursor| cursor.id)
        .expect("a parked cursor to step");
    let mut runtime = state.cursor_debug(target);
    runtime.paused = false;
    runtime.step_requested = true;
    state.set_cursor_debug(target, runtime);
    run.state = state.to_state();
    run.status = WorkflowStatus::Running;
    store.insert_run(run);
}

// the whole point: `step_all` must stop *before* executing the first node. the debugger used to
// persist this intent and the reducer ran straight past it.
#[tokio::test]
async fn a_step_all_run_parks_before_its_first_node() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(linear()));
    store.insert_run(debug_run("step_all"));

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("drive");

    let state = state_of(&store);
    let cursor = state.primary_cursor().expect("a placed cursor");
    assert!(cursor.is_at("start"), "the cursor should still be on start");
    assert!(
        state.cursor_debug(cursor.id).paused,
        "a step_all run must park before executing anything"
    );
    assert!(
        store.latest_node_run("start").is_none(),
        "parking happens before the node runs, so it records no node run"
    );
    assert_eq!(
        store.run(RUN_ID.parse().unwrap()).expect("run").status,
        WorkflowStatus::DebugPaused,
        "with only one cursor, parking it pauses the run"
    );
}

// the snapshot fields (`current_node_id`, `context_json`, ...) had no writer at all, so the ui's
// inspection panes read from nothing.
#[tokio::test]
async fn parking_records_the_inspection_snapshot() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(linear()));
    store.insert_run(debug_run("step_all"));

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("drive");

    let state = state_of(&store);
    let runtime = state.cursor_debug(state.primary_cursor().expect("cursor").id);
    assert_eq!(runtime.current_node_id.as_deref(), Some("start"));
    assert!(
        runtime.current_node_kind.is_some(),
        "the ui shows the node kind beside its id"
    );
    assert!(
        runtime.context_json.is_some(),
        "watch expressions evaluate against the captured context"
    );
    assert!(
        state
            .debug
            .as_ref()
            .expect("frame")
            .runtime
            .current_node_id
            .is_some(),
        "the flat frame mirrors the primary cursor for single-position clients"
    );
}

// step consumes the request on entry, which is what makes it advance exactly one node rather than
// running to completion.
#[tokio::test]
async fn a_step_advances_exactly_one_node() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(linear()));
    store.insert_run(debug_run("step_all"));
    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("first drive");

    request_step(&store);
    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("second drive");

    let state = state_of(&store);
    let cursor = state.primary_cursor().expect("cursor");
    assert!(
        cursor.is_at("middle"),
        "one step should land on the next node, got {cursor}"
    );
    assert!(
        state.cursor_debug(cursor.id).paused,
        "and re-park there rather than running on"
    );
    assert!(
        store.latest_node_run("middle").is_none(),
        "the step executed `start`, then parked *before* `middle`"
    );
}

// in `breakpoints` mode the run should sail past unmarked nodes and stop only where asked.
#[tokio::test]
async fn breakpoints_mode_runs_to_the_marked_node() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(linear()));
    store.insert_run(run_with_state(serde_json::json!({
        "debug": { "enabled": true, "mode": "breakpoints", "breakpoints": ["middle"] }
    })));

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("drive");

    let state = state_of(&store);
    let cursor = state.primary_cursor().expect("cursor");
    assert!(
        cursor.is_at("middle"),
        "it should run through start and stop at the breakpoint, got {cursor}"
    );
    assert!(state.cursor_debug(cursor.id).paused);
    assert!(
        store.latest_node_run("start").is_some(),
        "`start` was not marked, so it executed"
    );
}

// a run with no debug frame must be completely unaffected -- the gate is the hot path for every
// non-debug run in the system.
#[tokio::test]
async fn a_run_without_a_debug_frame_is_untouched() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(linear()));
    store.insert_run(run_with_state(serde_json::json!({})));

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("drive");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_eq!(
        run.status,
        WorkflowStatus::Succeeded,
        "an undebugged run should walk straight to its end"
    );
    assert!(!WorkflowRunState::from_state(&run.state).all_cursors_paused());
}

// `enabled: false` is the same as no frame at all.
#[tokio::test]
async fn a_disabled_debug_frame_does_not_break() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(linear()));
    store.insert_run(run_with_state(serde_json::json!({
        "debug": { "enabled": false, "mode": "step_all" }
    })));

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("drive");

    assert_eq!(
        store.run(RUN_ID.parse().unwrap()).expect("run").status,
        WorkflowStatus::Succeeded
    );
}

// one branch stopping at a breakpoint must leave the run `Running`: its siblings are still going.
// gating the ui on the *run's* status is what would disable every debug button mid-fan-out.
#[tokio::test]
async fn one_parked_branch_leaves_the_run_running() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "fork" } } },
        { "id": "fork", "kind": "parallel",
          "parameters": { "branches": [{ "$node": "a" }, { "$node": "b" }] } },
        { "id": "a", "kind": "audit", "parameters": { "action": "a" },
          "transitions": { "on_success": { "$node": "join" } } },
        { "id": "b", "kind": "audit", "parameters": { "action": "b" },
          "transitions": { "on_success": { "$node": "join" } } },
        { "id": "join", "kind": "join",
          "parameters": { "wait_for": [{ "$node": "a" }, { "$node": "b" }], "mode": "all" },
          "transitions": { "on_success": { "$node": "done" } } },
        { "id": "done", "kind": "end" }
    ])));
    store.insert_run(run_with_state(serde_json::json!({
        "debug": { "enabled": true, "mode": "breakpoints", "breakpoints": ["a"] }
    })));

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("fan out");
    // drive the branch that carries the breakpoint.
    process_ready_node(&store, &ready_node("a"))
        .await
        .expect("drive branch a");

    let state = state_of(&store);
    let parked = state
        .cursors
        .iter()
        .find(|cursor| cursor.is_at("a"))
        .expect("branch a still holds a cursor");
    assert!(
        state.cursor_debug(parked.id).paused,
        "branch a should be parked at its breakpoint"
    );
    assert!(
        !state.all_cursors_paused(),
        "branch b is not parked, so not every cursor is"
    );
    assert_ne!(
        store.run(RUN_ID.parse().unwrap()).expect("run").status,
        WorkflowStatus::DebugPaused,
        "the run keeps running while any branch can still advance"
    );
}

// the bug fix pinned: `should_stop_inline_progress` and `active_node_awaits_worker` used to read
// `active_node_id` -- the *primary* cursor's mirror -- so a drive following branch B judged itself
// against branch A's parked action and bailed after a single step. B then advanced one node per
// broker round trip instead of up to 64.
#[tokio::test]
async fn a_drive_is_not_stopped_by_a_sibling_parked_on_an_action() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "fork" } } },
        { "id": "fork", "kind": "parallel",
          "parameters": { "branches": [{ "$node": "slow" }, { "$node": "b1" }] } },
        // a worker-bound action: parks Running and does not settle inline.
        { "id": "slow", "kind": "action",
          "action": { "provider": "test", "function": "wait", "configuration": {} },
          "transitions": { "on_success": { "$node": "join" } } },
        // a chain of in-process nodes: with the bug, only the first one runs per drive.
        { "id": "b1", "kind": "audit", "parameters": { "action": "1" },
          "transitions": { "on_success": { "$node": "b2" } } },
        { "id": "b2", "kind": "audit", "parameters": { "action": "2" },
          "transitions": { "on_success": { "$node": "b3" } } },
        { "id": "b3", "kind": "audit", "parameters": { "action": "3" },
          "transitions": { "on_success": { "$node": "join" } } },
        { "id": "join", "kind": "join",
          "parameters": { "wait_for": [{ "$node": "slow" }, { "$node": "b3" }], "mode": "all" },
          "transitions": { "on_success": { "$node": "done" } } },
        { "id": "done", "kind": "end" }
    ])));
    store.insert_run(run_with_state(serde_json::json!({})));

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("fan out");
    // park the action branch first so it becomes the run's mirrored position.
    process_ready_node(&store, &ready_node("slow"))
        .await
        .expect("drive the action branch");
    process_ready_node(&store, &ready_node("b1"))
        .await
        .expect("drive the fast branch");

    for node_id in ["b1", "b2", "b3"] {
        assert!(
            store.latest_node_run(node_id).is_some(),
            "{node_id} should have run: one drive follows its own cursor all the way, and must \
             not stop because a *sibling* is parked on an action"
        );
    }
}

// a speculative branch must not be able to act on the outside world.
#[tokio::test]
async fn a_speculative_cursor_shadows_an_action_instead_of_dispatching_it() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "call" } } },
        { "id": "call", "kind": "action",
          "action": { "provider": "test", "function": "ship_it", "configuration": {} },
          "transitions": { "on_success": { "$node": "done" } } },
        { "id": "done", "kind": "end" }
    ])));
    let mut run =
        run_with_state(serde_json::json!({ "debug": { "enabled": true, "mode": "breakpoints" } }));
    // place a real cursor, then fork a speculative branch from it onto the action.
    let mut state = WorkflowRunState::from_state(&run.state);
    let real = state.ensure_cursor("done");
    let spec = state
        .fork_speculative(real, "call", Some("what-if".into()), Value::Null)
        .expect("fork");
    run.state = state.to_state();
    run.status = WorkflowStatus::Running;
    store.insert_run(run);

    let mut ready = ready_node("call");
    ready.cursor_id = Some(spec);
    process_ready_node(&store, &ready)
        .await
        .expect("drive the speculative branch");

    assert!(
        store.dispatches().is_empty(),
        "a speculative cursor must not dispatch a real action, got {:?}",
        store.dispatches()
    );
    let recorded = store.latest_node_run("call").expect("a shadow node run");
    assert!(
        recorded.speculative,
        "the shadowed run must be tagged so it stays out of a real branch's context"
    );
    assert_eq!(recorded.cursor_id, Some(spec));
}

// arming is the deliberate opt-out from shadowing, one node at a time.
#[tokio::test]
async fn an_armed_node_dispatches_for_real() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "call" } } },
        { "id": "call", "kind": "action",
          "action": { "provider": "test", "function": "ship_it", "configuration": {} },
          "transitions": { "on_success": { "$node": "done" } } },
        { "id": "done", "kind": "end" }
    ])));
    let mut run =
        run_with_state(serde_json::json!({ "debug": { "enabled": true, "mode": "breakpoints" } }));
    let mut state = WorkflowRunState::from_state(&run.state);
    let real = state.ensure_cursor("done");
    let spec = state
        .fork_speculative(real, "call", None, Value::Null)
        .expect("fork");
    state
        .cursor_mut(spec)
        .expect("spec")
        .speculative
        .as_mut()
        .expect("frame")
        .armed_nodes
        .insert("call".into());
    run.state = state.to_state();
    run.status = WorkflowStatus::Running;
    store.insert_run(run);

    let mut ready = ready_node("call");
    ready.cursor_id = Some(spec);
    process_ready_node(&store, &ready)
        .await
        .expect("drive the armed branch");

    assert_eq!(
        store.dispatches().len(),
        1,
        "an armed node is the explicit opt-in to real execution"
    );
}

// a hypothetical failure must not take the real work down with it.
#[tokio::test]
async fn a_failing_speculative_branch_leaves_the_real_run_alone() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "boom" } } },
        { "id": "boom", "kind": "fail" },
        { "id": "done", "kind": "end" }
    ])));
    let mut run =
        run_with_state(serde_json::json!({ "debug": { "enabled": true, "mode": "breakpoints" } }));
    let mut state = WorkflowRunState::from_state(&run.state);
    let real = state.ensure_cursor("done");
    let spec = state
        .fork_speculative(real, "boom", None, Value::Null)
        .expect("fork");
    run.state = state.to_state();
    run.status = WorkflowStatus::Running;
    store.insert_run(run);

    let mut ready = ready_node("boom");
    ready.cursor_id = Some(spec);
    process_ready_node(&store, &ready)
        .await
        .expect("drive the doomed branch");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_ne!(
        run.status,
        WorkflowStatus::Failed,
        "a speculative failure is a hypothetical; it must not fail the run"
    );
    assert_eq!(
        run.status,
        WorkflowStatus::Succeeded,
        "the real thread of control carried on to its end regardless"
    );
    let state = WorkflowRunState::from_state(&run.state);
    assert!(
        state.cursor(spec).is_none(),
        "the speculative branch drains itself"
    );
    // `real` retired normally by finishing, not by being drained with the failure -- which is the
    // distinction that matters: without the speculative guard, `advance_cursor`'s failing-terminal
    // arm would have cleared every cursor and marked the whole run Failed.
    assert!(state.cursors.is_empty());
}

// a blocked thread of control is stuck, not finished. retiring it would leave a live run with no
// cursor to drive and silently discard the loop/try frames that say where it was.
#[tokio::test]
async fn a_blocked_node_keeps_its_cursor_in_place() {
    let store = FakeStore::new();
    // the reentry safety bound with no `on_exhausted` target is a real `block_node` caller.
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "capped" } } },
        { "id": "capped", "kind": "audit",
          "parameters": { "action": "noted" },
          "reentry": { "enabled": true, "max_visits": 1 },
          "transitions": { "on_success": { "$node": "done" } } },
        { "id": "done", "kind": "end" }
    ])));
    store.insert_run(run_with_state(serde_json::json!({})));
    // a prior completed visit, so entering it again is already over the bound.
    store.insert_node_run(
        serde_json::from_value(serde_json::json!({
            "id": Uuid::now_v7(),
            "workflow_run_id": RUN_ID,
            "node_id": "capped",
            "status": "succeeded",
            "attempt": 1,
            "parameters": {},
            "output_json": null,
            "state": null,
            "transition_reason": null,
            "created_at": Utc::now(),
            "started_at": null,
            "finished_at": Utc::now(),
            "message": null,
        }))
        .expect("prior visit"),
    );

    process_ready_node(&store, &ready_node("capped"))
        .await
        .expect("drive");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_eq!(run.status, WorkflowStatus::Blocked);
    let state = WorkflowRunState::from_state(&run.state);
    let cursor = state
        .primary_cursor()
        .expect("a blocked run keeps the cursor so it can be inspected and retried");
    assert!(
        cursor.is_at("capped"),
        "it stays on the node it blocked at, got {cursor}"
    );
}

// a short-circuiting cooldown finishes its own branch. writing `Succeeded` on the run directly --
// as it used to -- would end the whole run while sibling branches were still executing.
#[tokio::test]
async fn a_terminal_settles_through_the_cursor_and_drains_it() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(linear()));
    store.insert_run(run_with_state(serde_json::json!({})));

    process_ready_node(&store, &ready_node("start"))
        .await
        .expect("drive");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    assert!(
        WorkflowRunState::from_state(&run.state).cursors.is_empty(),
        "a finished run holds no threads of control"
    );
}

// a nested fork continues its parent's exploration, so the parent's recorded work is its history.
// reading the *subtree* instead would hide that and show it divergent branches it never took.
#[test]
fn a_nested_fork_sees_its_parents_work_and_not_its_siblings() {
    let mut state = WorkflowRunState::default();
    let real = state.ensure_cursor("gate");
    let parent = state
        .fork_speculative(real, "gate", None, Value::Null)
        .expect("parent fork");
    let child = state
        .fork_speculative(parent, "gate", None, Value::Null)
        .expect("nested fork");
    let sibling = state
        .fork_speculative(real, "gate", None, Value::Null)
        .expect("sibling fork");

    let node_runs: Vec<runinator_models::workflows::WorkflowNodeRun> = serde_json::from_value(
        serde_json::json!([
            { "id": Uuid::now_v7(), "workflow_run_id": RUN_ID, "node_id": "call",
              "status": "succeeded", "attempt": 1, "parameters": {}, "output_json": { "from": "parent" },
              "state": null, "transition_reason": null, "created_at": Utc::now(),
              "started_at": null, "finished_at": null, "message": null,
              "speculative": true, "cursor_id": parent },
            { "id": Uuid::now_v7(), "workflow_run_id": RUN_ID, "node_id": "call",
              "status": "succeeded", "attempt": 1, "parameters": {}, "output_json": { "from": "sibling" },
              "state": null, "transition_reason": null, "created_at": Utc::now(),
              "started_at": null, "finished_at": null, "message": null,
              "speculative": true, "cursor_id": sibling },
        ]),
    )
    .expect("node runs");

    let child_cursor = state.cursor(child).expect("child").clone();
    let visible = super::context::visible_node_runs(&child_cursor, &state, &node_runs);
    assert_eq!(
        visible.len(),
        1,
        "exactly one of the two is this fork's history"
    );
    assert_eq!(visible[0].cursor_id, Some(parent));
}

// isolation in the direction that matters: a real branch must never read a hypothetical's output.
#[test]
fn a_real_cursor_cannot_see_speculative_output() {
    let mut state = WorkflowRunState::default();
    let real = state.ensure_cursor("gate");
    let spec = state
        .fork_speculative(real, "gate", None, Value::Null)
        .expect("fork");

    let node_runs: Vec<runinator_models::workflows::WorkflowNodeRun> = serde_json::from_value(
        serde_json::json!([
            { "id": Uuid::now_v7(), "workflow_run_id": RUN_ID, "node_id": "call",
              "status": "succeeded", "attempt": 1, "parameters": {}, "output_json": { "real": true },
              "state": null, "transition_reason": null, "created_at": Utc::now(),
              "started_at": null, "finished_at": null, "message": null, "speculative": false },
            { "id": Uuid::now_v7(), "workflow_run_id": RUN_ID, "node_id": "call",
              "status": "succeeded", "attempt": 1, "parameters": {}, "output_json": { "real": false },
              "state": null, "transition_reason": null, "created_at": Utc::now(),
              "started_at": null, "finished_at": null, "message": null,
              "speculative": true, "cursor_id": spec },
        ]),
    )
    .expect("node runs");

    let real_cursor = state.cursor(real).expect("real").clone();
    let visible = super::context::visible_node_runs(&real_cursor, &state, &node_runs);
    assert_eq!(visible.len(), 1, "a real cursor sees only real output");
    assert!(!visible[0].speculative);

    let spec_cursor = state.cursor(spec).expect("spec").clone();
    let visible = super::context::visible_node_runs(&spec_cursor, &state, &node_runs);
    assert_eq!(
        visible.len(),
        2,
        "a speculative cursor sees the real run *and* its own subtree"
    );
}

// the "what if this value were different" case: the patch reaches nested context paths.
#[test]
fn a_context_patch_overlays_nested_paths() {
    let mut target = serde_json::json!({
        "steps": { "fetch": { "output": { "status": 200, "body": "ok" } } },
        "input": { "retries": 1 }
    })
    .into();
    let patch: Value = serde_json::json!({
        "steps": { "fetch": { "output": { "status": 403 } } }
    })
    .into();

    super::context::deep_merge(&mut target, &patch);

    assert_eq!(
        target.pointer("/steps/fetch/output/status"),
        Some(&Value::from(403)),
        "the patched leaf wins"
    );
    assert_eq!(
        target.pointer("/steps/fetch/output/body"),
        Some(&Value::from("ok")),
        "siblings of the patched leaf survive -- a shallow merge would have dropped `body`"
    );
    assert_eq!(target.pointer("/input/retries"), Some(&Value::from(1)));
}

// a run paused by the previous single-cursor debugger has no per-cursor runtime; it must resume
// with its state intact rather than reading as "not paused".
#[test]
fn a_legacy_paused_run_still_reads_as_paused() {
    let state = WorkflowRunState::from_state(
        &serde_json::json!({
            "debug": { "enabled": true, "paused": true, "current_node_id": "middle" },
            "cursors": [{ "id": Uuid::now_v7(), "node_id": "middle" }]
        })
        .into(),
    );
    let cursor = state.primary_cursor().expect("cursor");
    assert!(
        cursor.debug.is_none(),
        "the fixture predates per-cursor state"
    );
    assert!(
        state.cursor_debug(cursor.id).paused,
        "it must fall back to the run frame, or the run silently un-pauses on upgrade"
    );
}

// once any cursor carries its own runtime the flat frame is the *primary's mirror*, not the run's
// state, so a sibling without one must not inherit it.
#[test]
fn a_sibling_without_a_runtime_does_not_inherit_the_mirror() {
    let mut state = WorkflowRunState::default();
    state.debug = Some(Default::default());
    let first = state.ensure_cursor("a");
    let second = state.fork_cursor("b", "fork");
    state.set_cursor_debug(
        first,
        DebugRuntime {
            paused: true,
            ..Default::default()
        },
    );

    assert!(state.cursor_debug(first).paused);
    assert!(
        !state.cursor_debug(second).paused,
        "branch b was never parked; reading the mirror would freeze it by accident"
    );
}

// a fork made inside a loop iteration explores from where its parent stands.
#[test]
fn a_fork_inherits_its_parents_position_and_frames() {
    let parent = {
        let mut cursor = RunCursor::at("body");
        cursor.loop_frame = Some(runinator_models::workflow_state::LoopFrame {
            index: 4,
            item: Value::from("item-4"),
            return_to: "each".into(),
        });
        cursor
    };
    let fork = RunCursor::speculative_from(&parent, parent.node_id(), None, Value::Null);

    assert!(fork.is_at("body"));
    assert_eq!(fork.loop_frame.as_ref().map(|frame| frame.index), Some(4));
    assert_eq!(
        fork.speculative
            .as_ref()
            .map(|frame| frame.forked_from_cursor),
        Some(parent.id)
    );
}
