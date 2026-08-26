//! covers the interrupt vocabulary and the compat guarantees the run-state blob depends on.

use super::*;
use crate::cursor::RunCursor;
use crate::json;
use crate::workflow_state::{DebugRuntime, WorkflowExecutionState};

fn frame(interrupted: Uuid) -> InterruptFrame {
    InterruptFrame {
        interrupted_cursor: interrupted,
        source: InterruptSource::Wake,
        payload: json!({ "deadline_unix": 42 }),
        resume: ResumePoint {
            node_id: "poll".into(),
            loops: Vec::new(),
            try_frame: None,
        },
        raised_at: Utc::now(),
    }
}

#[test]
fn source_and_mode_names_round_trip() {
    for source in InterruptSource::ALL {
        assert_eq!(source.as_str().parse(), Ok(source));
    }
    for mode in InterruptMode::ALL {
        assert_eq!(mode.as_str().parse(), Ok(mode));
    }
    assert!("webhook".parse::<InterruptSource>().is_err());
    assert!("abort".parse::<InterruptMode>().is_err());
}

/// the serde names are a wire contract shared with the rexrap front end and the command center, so
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

/// `ALL` drives source matching, the rexrap keyword list, and the docs. a variant missing from it is
/// simply never raised, which is silent — so pin the count and the serde names together.
#[test]
fn every_source_is_listed_exactly_once_with_its_wire_name() {
    let names: Vec<&str> = InterruptSource::ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        names,
        [
            "external",
            "orphan_signal",
            "wake",
            "timeout",
            "retry",
            "failure",
            "resolved",
            "child",
        ],
        "ALL is the precedence order the reducer matches in; changing it changes which source wins"
    );
    for source in InterruptSource::ALL {
        assert_eq!(
            serde_json::to_value(source).unwrap(),
            serde_json::Value::String(source.as_str().into()),
            "the serde name is the author-facing name"
        );
    }
}

/// the mode list is what the catalog offers and what the `resume` node validates against, so a
/// variant missing from it is a mode the UI cannot pick and the node rejects.
#[test]
fn every_resume_mode_is_listed_exactly_once_with_its_wire_name() {
    let names: Vec<&str> = InterruptMode::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(names, ["resume", "continue", "restart", "fail"]);
    for mode in InterruptMode::ALL {
        assert_eq!(
            serde_json::to_value(mode).unwrap(),
            serde_json::Value::String(mode.as_str().into())
        );
    }
}

/// a requested source has no node state to match against, so the reducer must never look for one on
/// a drive; a drive-matched source must never sit waiting in the pending queue.
#[test]
fn only_the_out_of_band_sources_are_requested() {
    let requested: Vec<&str> = InterruptSource::ALL
        .iter()
        .filter(|source| source.requested())
        .map(|source| source.as_str())
        .collect();
    assert_eq!(requested, ["external", "orphan_signal"]);
}

#[test]
fn a_pending_request_is_taken_oldest_first_by_its_target() {
    let mine = Uuid::now_v7();
    let other = Uuid::now_v7();
    let mut state = WorkflowExecutionState::default();
    let mut newer = PendingInterrupt::new(InterruptSource::External, json!({ "n": 2 }), None);
    newer.requested_at = Utc::now();
    let mut older = PendingInterrupt::new(InterruptSource::External, json!({ "n": 1 }), None);
    older.requested_at = Utc::now() - chrono::Duration::seconds(30);
    let older_id = older.id;
    let mut targeted = PendingInterrupt::new(InterruptSource::External, Value::Null, Some(other));
    targeted.requested_at = Utc::now() - chrono::Duration::seconds(60);
    let targeted_id = targeted.id;
    state.pending_interrupts = vec![newer, older, targeted];

    assert_eq!(
        state.pending_interrupt_for(mine).map(|request| request.id),
        Some(older_id),
        "a burst is served in the order it was made, and never from another thread's request"
    );
    assert_eq!(
        state.pending_interrupt_for(other).map(|request| request.id),
        Some(targeted_id),
        "a request naming this thread is visible to it"
    );

    assert!(state.take_pending_interrupt(older_id));
    assert!(
        !state.take_pending_interrupt(older_id),
        "a duplicated drive must be able to tell it already decided"
    );
    assert_eq!(state.pending_interrupts.len(), 2);
}

#[test]
fn a_run_with_no_pending_interrupts_serializes_exactly_as_before() {
    let encoded = serde_json::to_value(WorkflowExecutionState::default()).unwrap();
    assert!(
        !encoded
            .as_object()
            .expect("state encodes as an object")
            .contains_key("pending_interrupts"),
        "the key must not appear on runs nobody interrupts, or every persisted run churns"
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
    assert!(
        declaration.enabled,
        "legacy declarations default to enabled"
    );
}

#[test]
fn a_disabled_interrupt_link_round_trips_explicitly() {
    let declaration: InterruptDeclaration = serde_json::from_value(serde_json::json!({
        "on": "wake", "handler": "on_wake", "enabled": false
    }))
    .expect("a disabled declaration parses");
    assert!(!declaration.enabled);
    assert_eq!(
        serde_json::to_value(declaration).unwrap().get("enabled"),
        Some(&serde_json::json!(false))
    );
}

/// The frame is made structurally incapable of failing to parse: `WorkflowExecutionState::from_state`
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
    let state = WorkflowExecutionState::from_state(&legacy);
    let cursor = state.primary_cursor().expect("the legacy cursor survives");
    assert!(!cursor.is_suspended());
    assert!(!cursor.is_interrupt_handler());
    assert!(cursor.handled.is_empty());
}

#[test]
fn moving_a_cursor_forgets_which_interrupts_fired_there() {
    let mut cursor = RunCursor::at("poll");
    cursor.mark_handled(handled_key(InterruptSource::Wake, Uuid::now_v7(), 0));
    assert!(!cursor.handled.is_empty());

    // re-entering the *same* node keeps the record: that is what stops a `resume` re-raising.
    cursor.move_to("poll");
    assert!(!cursor.handled.is_empty());

    cursor.move_to("next");
    assert!(cursor.handled.is_empty());
}

#[test]
fn retiring_a_cursor_also_retires_its_handler() {
    let mut state = WorkflowExecutionState::default();
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
    let mut state = WorkflowExecutionState::default();
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
    let mut state = WorkflowExecutionState::default();
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
