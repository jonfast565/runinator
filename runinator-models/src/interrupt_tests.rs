//! covers the interrupt vocabulary and the compat guarantees the run-state blob depends on.

use super::*;
use crate::cursor::RunCursor;
use crate::json;
use crate::workflow_state::{DebugRuntime, WorkflowRunState};

fn frame(interrupted: Uuid) -> InterruptFrame {
    InterruptFrame {
        interrupted_cursor: interrupted,
        source: InterruptSource::Wake,
        payload: json!({ "deadline_unix": 42 }),
        resume: ResumePoint {
            node_id: "poll".into(),
            loop_frame: None,
            try_frame: None,
        },
        raised_at: Utc::now(),
    }
}

#[test]
fn source_and_mode_names_round_trip() {
    for source in [InterruptSource::Wake] {
        assert_eq!(InterruptSource::from_str(source.as_str()), Some(source));
    }
    for mode in [
        InterruptMode::Resume,
        InterruptMode::Continue,
        InterruptMode::Restart,
        InterruptMode::Fail,
    ] {
        assert_eq!(InterruptMode::from_str(mode.as_str()), Some(mode));
    }
    assert_eq!(InterruptSource::from_str("webhook"), None);
    assert_eq!(InterruptMode::from_str("abort"), None);
}

/// the serde names are a wire contract shared with the wdl front end and the command center, so
/// pin them against literals rather than against a round trip that would agree with itself.
#[test]
fn source_and_mode_serialize_to_their_author_facing_names() {
    assert_eq!(
        serde_json::to_value(InterruptSource::Wake).unwrap(),
        serde_json::json!("wake")
    );
    assert_eq!(
        serde_json::to_value(InterruptMode::Restart).unwrap(),
        serde_json::json!("restart")
    );
}

/// a declaration naming a source this binary does not know must not fail the definition parse —
/// the runtime simply never matches it, which is the fail-open rule.
#[test]
fn an_unknown_source_parses_but_resolves_to_nothing() {
    let declaration: InterruptDeclaration =
        serde_json::from_value(serde_json::json!({ "on": "webhook", "handler": "on_hook" }))
            .expect("unknown sources must still deserialize");
    assert_eq!(declaration.source(), None);
}

/// the frame is made structurally incapable of failing to parse: `WorkflowRunState::from_state`
/// falls back to `unwrap_or_default`, so a strict frame would discard every cursor in the run.
#[test]
fn a_truncated_interrupt_frame_still_parses() {
    let frame: InterruptFrame =
        serde_json::from_value(serde_json::json!({})).expect("every field of the frame defaults");
    assert_eq!(frame.source, InterruptSource::Wake);
    assert_eq!(frame.resume.node_id, "");
}

#[test]
fn an_uninterrupted_cursor_serializes_exactly_as_before() {
    let cursor = RunCursor::at("start");
    let encoded = serde_json::to_value(&cursor).unwrap();
    let object = encoded.as_object().expect("a cursor encodes as an object");
    for absent in ["interrupt", "suspended_by", "handled"] {
        assert!(
            !object.contains_key(absent),
            "'{absent}' must not appear on an uninterrupted cursor, or every persisted run churns"
        );
    }
}

#[test]
fn a_state_blob_without_the_interrupt_keys_still_loads() {
    let legacy = json!({
        "cursors": [{ "id": Uuid::now_v7().to_string(), "node_id": "poll" }]
    });
    let state = WorkflowRunState::from_state(&legacy);
    let cursor = state.primary_cursor().expect("the legacy cursor survives");
    assert!(!cursor.is_suspended());
    assert!(!cursor.is_interrupt_handler());
    assert!(cursor.handled.is_empty());
}

#[test]
fn moving_a_cursor_forgets_which_interrupts_fired_there() {
    let mut cursor = RunCursor::at("poll");
    cursor.mark_handled(handled_key(InterruptSource::Wake, Uuid::now_v7()));
    assert!(!cursor.handled.is_empty());

    // re-entering the *same* node keeps the record: that is what stops a `resume` re-raising.
    cursor.move_to("poll");
    assert!(!cursor.handled.is_empty());

    cursor.move_to("next");
    assert!(cursor.handled.is_empty());
}

#[test]
fn retiring_a_cursor_also_retires_its_handler() {
    let mut state = WorkflowRunState::default();
    let interrupted = state.ensure_cursor("poll");
    let handler = RunCursor::interrupt_handler("refresh", frame(interrupted));
    let handler_id = handler.id;
    state.cursors.push(handler);

    assert!(state.handler_for(interrupted).is_some());
    state.retire_cursor(interrupted);

    assert!(
        state.cursor(handler_id).is_none(),
        "a handler with nowhere to return to would pin the run open forever"
    );
    assert!(state.cursors.is_empty());
}

#[test]
fn a_handler_is_not_a_joinable_sibling_but_does_keep_the_run_alive() {
    let mut state = WorkflowRunState::default();
    let interrupted = state.ensure_cursor("poll");
    state
        .cursors
        .push(RunCursor::interrupt_handler("refresh", frame(interrupted)));

    assert_eq!(
        state.joinable_cursors().count(),
        1,
        "counting a handler as a branch would make a lone branch retire into a stall"
    );
    assert_eq!(
        state.real_cursors().count(),
        2,
        "the run is not finished while a handler is executing"
    );
}

#[test]
fn a_suspended_cursor_does_not_hold_the_debugger_out_of_paused() {
    let mut state = WorkflowRunState::default();
    let interrupted = state.ensure_cursor("poll");
    let handler = RunCursor::interrupt_handler("refresh", frame(interrupted));
    let handler_id = handler.id;
    state.cursors.push(handler);

    state
        .cursor_mut(interrupted)
        .expect("the interrupted cursor is present")
        .suspended_by = Some(handler_id);
    state.set_cursor_debug(
        handler_id,
        DebugRuntime {
            paused: true,
            ..Default::default()
        },
    );

    assert!(
        state.all_cursors_paused(),
        "a frozen cursor cannot reach a breakpoint, so it must not veto DebugPaused"
    );
}

/// a speculative fork of a suspended cursor must not be born frozen, nor inherit handler identity.
#[test]
fn a_speculative_fork_does_not_inherit_the_interrupt_fields() {
    let mut parent = RunCursor::at("poll");
    parent.suspended_by = Some(Uuid::now_v7());
    parent.mark_handled("wake:whatever");

    let fork = RunCursor::speculative_from(&parent, "poll", None, Value::Null);

    assert!(!fork.is_suspended());
    assert!(!fork.is_interrupt_handler());
    assert!(fork.handled.is_empty());
}
