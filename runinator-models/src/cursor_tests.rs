//! covers [`RunCursor`]: the placed/unplaced distinction, the start-node fallback, and the
//! per-cursor frames that keep two concurrent branches from sharing one thread of control.

use super::*;
use crate::value::Value;

// a minimal run; `active_node_id` is the only field these tests vary.
fn run(active_node_id: Option<&str>) -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "workflow_id": "00000000-0000-0000-0000-000000000002",
        "status": "running",
        "active_node_id": active_node_id,
        "parameters": {},
        "created_at": "2026-01-01T00:00:00Z",
        "started_at": null,
        "finished_at": null,
        "message": null,
    }))
    .expect("run fixture")
}

#[test]
fn of_reads_a_placed_run() {
    let cursor = RunCursor::of(&run(Some("verify"))).expect("placed");
    assert!(cursor.is_at("verify"));
}

#[test]
fn of_reports_an_unplaced_run() {
    assert_eq!(RunCursor::of(&run(None)), None);
}

#[test]
fn resolve_keeps_a_placed_run_where_it_is() {
    let cursor = RunCursor::resolve(&run(Some("verify")), "start");
    assert!(cursor.is_at("verify"));
    assert!(!cursor.is_at("start"));
}

#[test]
fn resolve_falls_back_to_start_for_an_unplaced_run() {
    assert!(RunCursor::resolve(&run(None), "start").is_at("start"));
}

#[test]
fn node_id_round_trips_through_the_cursor() {
    let cursor = RunCursor::at("verify");
    assert_eq!(cursor.node_id(), "verify");
    assert_eq!(cursor.to_string(), "verify");
    assert_eq!(cursor.into_node_id(), "verify");
}

#[test]
fn every_cursor_gets_its_own_identity() {
    assert_ne!(RunCursor::at("verify").id, RunCursor::at("verify").id);
}

#[test]
fn a_forked_cursor_records_the_node_that_forked_it() {
    let cursor = RunCursor::forked("branch_a", "fanout");
    assert!(cursor.is_at("branch_a"));
    assert_eq!(cursor.forked_by.as_deref(), Some("fanout"));
    assert_eq!(RunCursor::at("branch_a").forked_by, None);
}

fn loop_frame(node_id: &str, index: i64) -> LoopFrame {
    LoopFrame {
        node_id: node_id.into(),
        index,
        last_node_run_id: None,
        ..Default::default()
    }
}

#[test]
fn moving_a_cursor_leaves_its_frames_alone() {
    let mut cursor = RunCursor::at("body");
    cursor.set_loop_frame(loop_frame("each", 3));

    cursor.move_to("next");

    assert!(cursor.is_at("next"));
    assert_eq!(cursor.loop_frame("each").map(|frame| frame.index), Some(3));
}

// nested loops share a cursor, so one frame slot meant the inner loop overwrote the outer one's
// position. a keyed stack is what lets each find its own lap.
#[test]
fn nested_loops_keep_one_frame_each() {
    let mut cursor = RunCursor::at("body");
    cursor.set_loop_frame(loop_frame("outer", 1));
    cursor.set_loop_frame(loop_frame("inner", 2));

    assert_eq!(cursor.loop_frame("outer").map(|frame| frame.index), Some(1));
    assert_eq!(cursor.loop_frame("inner").map(|frame| frame.index), Some(2));
}

// a new outer lap has to restart the inner loop rather than resume it mid-count, so re-entering a
// loop drops whatever its body stacked on top.
#[test]
fn re_entering_a_loop_discards_the_frames_stacked_above_it() {
    let mut cursor = RunCursor::at("body");
    cursor.set_loop_frame(loop_frame("outer", 0));
    cursor.set_loop_frame(loop_frame("inner", 2));

    cursor.set_loop_frame(loop_frame("outer", 1));

    assert_eq!(cursor.loop_frame("outer").map(|frame| frame.index), Some(1));
    assert!(
        cursor.loop_frame("inner").is_none(),
        "the inner loop must restart on the next outer lap, not resume at index 2"
    );
}

#[test]
fn exiting_a_loop_drops_it_and_anything_nested() {
    let mut cursor = RunCursor::at("body");
    cursor.set_loop_frame(loop_frame("outer", 1));
    cursor.set_loop_frame(loop_frame("inner", 0));

    cursor.exit_loop("outer");

    assert!(cursor.loops.is_empty());
}

// the production bug this guards: the loop handler used to clear *every* frame on the cursor each
// lap, so a loop inside a try body silently reset the try phase to "body" on every iteration and
// the catch/finally arms became unreachable.
#[test]
fn loop_bookkeeping_leaves_the_try_frame_alone() {
    let mut cursor = RunCursor::at("body");
    cursor.try_frame = Some(TryFrame {
        node_id: "guard".into(),
        phase: "finally".into(),
        pending_status: None,
        pending_output: None,
    });

    cursor.set_loop_frame(loop_frame("each", 0));
    cursor.set_loop_frame(loop_frame("each", 1));
    cursor.exit_loop("each");

    assert_eq!(
        cursor.try_frame.as_ref().map(|frame| frame.phase.as_str()),
        Some("finally"),
        "a loop inside a try must not reset the try phase"
    );
    assert!(
        cursor.is_at("body"),
        "loop bookkeeping must not move the cursor"
    );
}

// a fork made inside a loop iteration or a try phase must explore from where the parent actually
// stands. inheriting the frames is what makes "fork here" mean here, not "restart the region".
#[test]
fn a_speculative_fork_inherits_its_parents_frames() {
    let mut parent = RunCursor::at("body");
    parent.set_loop_frame(loop_frame("outer", 1));
    parent.set_loop_frame(loop_frame("each", 2));

    let fork = RunCursor::speculative_from(&parent, "body", Some("what-if".into()), Value::Null);

    assert!(fork.is_speculative());
    assert_ne!(fork.id, parent.id);
    assert_eq!(fork.loop_frame("each").map(|frame| frame.index), Some(2));
    assert_eq!(
        fork.loop_frame("outer").map(|frame| frame.index),
        Some(1),
        "the whole nesting stack is inherited, not just the innermost loop"
    );
    let frame = fork.speculative.as_ref().expect("speculative frame");
    assert_eq!(frame.forked_from_cursor, parent.id);
    assert_eq!(frame.label.as_deref(), Some("what-if"));
}

// shadowing is the default so a "what if" branch cannot post the slack message for real. arming is
// the deliberate opt-out, one node at a time.
#[test]
fn only_armed_nodes_escape_shadowing_on_a_speculative_cursor() {
    let parent = RunCursor::at("call_api");
    let mut fork = RunCursor::speculative_from(&parent, "call_api", None, Value::Null);

    assert!(!fork.is_armed_for("call_api"));
    fork.speculative
        .as_mut()
        .expect("frame")
        .armed_nodes
        .insert("call_api".into());
    assert!(fork.is_armed_for("call_api"));
    assert!(!fork.is_armed_for("notify"));
}

#[test]
fn a_real_cursor_is_armed_for_everything() {
    let cursor = RunCursor::at("call_api");
    assert!(!cursor.is_speculative());
    assert!(cursor.is_armed_for("call_api"));
    assert!(cursor.is_armed_for("anything_else"));
}

#[test]
fn a_speculative_cursor_round_trips_with_its_frame() {
    let parent = RunCursor::at("call_api");
    let mut fork = RunCursor::speculative_from(
        &parent,
        "call_api",
        Some("what-if-403".into()),
        serde_json::json!({ "steps": { "fetch": { "output": { "status": 403 } } } }).into(),
    );
    fork.speculative
        .as_mut()
        .expect("frame")
        .armed_nodes
        .insert("call_api".into());
    fork.debug = Some(crate::workflow_state::DebugRuntime {
        paused: true,
        current_node_id: Some("call_api".into()),
        ..Default::default()
    });
    fork.last_output = Some(Value::from("prior"));

    let encoded = serde_json::to_value(&fork).expect("encode");
    let decoded: RunCursor = serde_json::from_value(encoded).expect("decode");

    assert_eq!(decoded, fork);
}

// the new fields are all optional, so a cursor persisted before they existed must still parse.
#[test]
fn a_cursor_persisted_before_these_fields_still_parses() {
    let decoded: RunCursor = serde_json::from_value(serde_json::json!({
        "id": "00000000-0000-0000-0000-00000000000c",
        "node_id": "verify",
    }))
    .expect("decode legacy cursor");

    assert!(decoded.is_at("verify"));
    assert!(!decoded.is_speculative());
    assert!(decoded.debug.is_none());
    assert!(decoded.last_output.is_none());
    assert!(decoded.visit_id.is_none());
    assert!(decoded.node_run_id.is_none());
}

#[test]
fn visit_identity_is_stable_until_the_cursor_moves() {
    let mut cursor = RunCursor::at("verify");
    let visit_id = cursor.ensure_visit();
    let node_run_id = Uuid::now_v7();
    cursor.attach_node_run(node_run_id);

    assert_eq!(cursor.ensure_visit(), visit_id);
    assert_eq!(cursor.node_run_id, Some(node_run_id));

    cursor.move_to("publish");

    assert!(cursor.visit_id.is_none());
    assert!(cursor.node_run_id.is_none());
    assert_ne!(cursor.ensure_visit(), visit_id);
}

#[test]
fn a_cursor_round_trips_with_its_identity_and_frames() {
    let mut cursor = RunCursor::forked("branch_a", "fanout");
    cursor.set_loop_frame(loop_frame("outer", 0));
    cursor.set_loop_frame(loop_frame("each", 1));

    let encoded = serde_json::to_value(&cursor).expect("encode");
    let decoded: RunCursor = serde_json::from_value(encoded).expect("decode");

    assert_eq!(decoded, cursor);
}

// the pre-stack single `"loop"` object is not read back: the field was renamed rather than
// reshaped, so the old key is an unknown field serde drops. a run caught mid-loop across the
// upgrade restarts that loop, which is the accepted cost of not carrying a compat shim — and
// crucially the *rest* of the cursor still parses, so the run does not lose its position.
#[test]
fn a_pre_stack_loop_key_is_ignored_without_failing_the_cursor() {
    let decoded: RunCursor = serde_json::from_value(serde_json::json!({
        "id": "00000000-0000-0000-0000-00000000000d",
        "node_id": "body",
        "loop": { "index": 3, "item": "x", "return_to": "each" },
    }))
    .expect("an unknown key must not fail the cursor");

    assert!(decoded.is_at("body"));
    assert!(decoded.loops.is_empty());
}
