//! covers [`WorkflowRunState`] as the run's consolidated state object: the cursors it carries, the
//! frames that used to be probed out of `extra`, the dynamic `event_source_<node_id>` keys, and the
//! tolerance that keeps one malformed frame from discarding a run's whole state.

use super::*;

fn state(json: serde_json::Value) -> WorkflowRunState {
    WorkflowRunState::from_state(&Value::from(json))
}

#[test]
fn a_run_with_no_cursors_is_seeded_once() {
    let mut parsed = WorkflowRunState::default();
    assert!(parsed.primary_cursor().is_none());

    let first = parsed.ensure_cursor("start");
    let again = parsed.ensure_cursor("somewhere_else");

    assert_eq!(
        first, again,
        "seeding an already-placed run must not re-place it"
    );
    assert_eq!(parsed.cursors.len(), 1);
    assert!(parsed.primary_cursor().expect("primary").is_at("start"));
}

#[test]
fn forked_cursors_are_addressable_by_id_and_by_origin() {
    let mut parsed = WorkflowRunState::default();
    parsed.ensure_cursor("fanout");
    let left = parsed.fork_cursor("branch_a", "fanout");
    let right = parsed.fork_cursor("branch_b", "fanout");

    assert_eq!(parsed.cursors.len(), 3);
    assert!(parsed.cursor(left).expect("left").is_at("branch_a"));
    assert!(parsed.cursor_at("branch_b").is_some());
    assert_eq!(parsed.cursors_forked_by("fanout").count(), 2);
    assert_eq!(parsed.cursors_forked_by("other").count(), 0);
    assert_ne!(left, right);
}

#[test]
fn retiring_a_cursor_reports_whether_it_was_still_live() {
    let mut parsed = WorkflowRunState::default();
    parsed.ensure_cursor("fanout");
    let branch = parsed.fork_cursor("branch_a", "fanout");

    assert!(
        parsed.retire_cursor(branch),
        "the first retirement takes it"
    );
    assert!(
        !parsed.retire_cursor(branch),
        "a repeat retirement is a no-op"
    );
    assert_eq!(parsed.cursors.len(), 1);
}

// a loop body used to re-enter by rebuilding the whole run state from default, which discarded
// every run-scoped key with it. resetting the cursor's own frames must leave the run alone.
#[test]
fn resetting_a_cursors_frames_preserves_run_scoped_state() {
    let mut parsed = state(serde_json::json!({
        "subflow_parent": { "run_id": "00000000-0000-0000-0000-00000000000a", "node_id": "parent" },
        "watch_fired": true,
    }));
    let id = parsed.ensure_cursor("body");
    parsed.cursor_mut(id).expect("cursor").loop_frame = Some(LoopFrame::default());

    parsed.cursor_mut(id).expect("cursor").clear_frames();

    assert!(parsed.cursor(id).expect("cursor").loop_frame.is_none());
    assert!(
        parsed.subflow_parent.is_some(),
        "run-scoped linkage must survive"
    );
    assert!(parsed.watch_fired, "run-scoped flags must survive");
}

#[test]
fn cursors_round_trip_through_the_state_blob() {
    let mut parsed = WorkflowRunState::default();
    parsed.ensure_cursor("fanout");
    parsed.fork_cursor("branch_a", "fanout");

    let reparsed = WorkflowRunState::from_state(&parsed.to_state());

    assert_eq!(reparsed.cursors, parsed.cursors);
}

// a speculative branch must be invisible to the machinery that decides what the run means: a join
// counting arrivals, a race deciding whether it has fanned out.
#[test]
fn speculative_cursors_are_excluded_from_fan_out_accounting() {
    let mut parsed = WorkflowRunState::default();
    let root = parsed.ensure_cursor("fanout");
    parsed.fork_cursor("branch_a", "fanout");
    let spec = parsed
        .fork_speculative(root, "branch_a", Some("what-if".into()), Value::Null)
        .expect("fork");
    parsed
        .cursor_mut(spec)
        .expect("spec")
        .forked_by
        .replace("fanout".into());

    assert_eq!(parsed.cursors.len(), 3);
    assert_eq!(
        parsed.cursors_forked_by("fanout").count(),
        1,
        "a speculative cursor must not look like an arrived branch"
    );
    assert_eq!(parsed.real_cursors().count(), 2);
    assert!(parsed.is_speculative(spec));
    assert!(!parsed.is_speculative(root));
}

// an abandoned fork must drain as a unit, or retiring the root strands its children forever.
#[test]
fn a_speculative_subtree_collects_nested_forks() {
    let mut parsed = WorkflowRunState::default();
    let root = parsed.ensure_cursor("start");
    let child = parsed
        .fork_speculative(root, "start", None, Value::Null)
        .expect("child");
    let grandchild = parsed
        .fork_speculative(child, "start", None, Value::Null)
        .expect("grandchild");
    let sibling = parsed
        .fork_speculative(root, "start", None, Value::Null)
        .expect("sibling");

    let subtree = parsed.speculative_subtree(child);
    assert!(subtree.contains(&child) && subtree.contains(&grandchild));
    assert!(
        !subtree.contains(&sibling),
        "an unrelated fork must not be drained"
    );
    assert_eq!(subtree.len(), 2);
}

// visibility walks *up* the fork chain and draining walks *down* it. getting these the same way
// round would either hide a fork's own history from it, or show it a path it never took.
#[test]
fn ancestry_and_subtree_walk_opposite_directions() {
    let mut parsed = WorkflowRunState::default();
    let root = parsed.ensure_cursor("start");
    let child = parsed
        .fork_speculative(root, "start", None, Value::Null)
        .expect("child");
    let grandchild = parsed
        .fork_speculative(child, "start", None, Value::Null)
        .expect("grandchild");

    let ancestry = parsed.speculative_ancestry(grandchild);
    assert!(
        ancestry.contains(&grandchild) && ancestry.contains(&child),
        "a fork's history includes the fork it continued from"
    );
    assert!(
        ancestry.contains(&root),
        "the walk stops at the real cursor it ultimately came from, inclusive"
    );

    let subtree = parsed.speculative_subtree(child);
    assert!(
        subtree.contains(&grandchild),
        "draining a fork takes the forks made from it"
    );
    assert!(
        !subtree.contains(&root),
        "draining a fork must never touch what it forked from"
    );
}

#[test]
fn forking_a_retired_cursor_reports_failure() {
    let mut parsed = WorkflowRunState::default();
    let root = parsed.ensure_cursor("start");
    parsed.retire_cursor(root);

    assert_eq!(
        parsed.fork_speculative(root, "start", None, Value::Null),
        None
    );
}

// the whole point of per-cursor runtime: stepping one branch must not step its siblings.
#[test]
fn each_cursor_carries_its_own_debugger_runtime() {
    let mut parsed = WorkflowRunState::default();
    parsed.debug = Some(DebugFrame::default());
    let left = parsed.ensure_cursor("branch_a");
    let right = parsed.fork_cursor("branch_b", "fanout");

    parsed.set_cursor_debug(
        left,
        DebugRuntime {
            paused: true,
            current_node_id: Some("branch_a".into()),
            ..Default::default()
        },
    );

    assert!(parsed.cursor_debug(left).paused);
    assert!(!parsed.cursor_debug(right).paused);
    assert!(
        !parsed.all_cursors_paused(),
        "one parked branch must leave the run running"
    );

    parsed.set_cursor_debug(
        right,
        DebugRuntime {
            paused: true,
            ..Default::default()
        },
    );
    assert!(parsed.all_cursors_paused());
}

// the flat frame is the wire contract single-position clients read, so it has to follow the primary.
#[test]
fn the_flat_frame_mirrors_the_primary_cursor() {
    let mut parsed = WorkflowRunState::default();
    parsed.debug = Some(DebugFrame::default());
    let primary = parsed.ensure_cursor("branch_a");
    let other = parsed.fork_cursor("branch_b", "fanout");

    parsed.set_cursor_debug(
        other,
        DebugRuntime {
            current_node_id: Some("branch_b".into()),
            ..Default::default()
        },
    );
    assert_eq!(
        parsed
            .debug
            .as_ref()
            .expect("frame")
            .runtime
            .current_node_id,
        None,
        "a non-primary cursor must not overwrite the mirror"
    );

    parsed.set_cursor_debug(
        primary,
        DebugRuntime {
            paused: true,
            current_node_id: Some("branch_a".into()),
            ..Default::default()
        },
    );
    let mirrored = &parsed.debug.as_ref().expect("frame").runtime;
    assert!(mirrored.paused);
    assert_eq!(mirrored.current_node_id.as_deref(), Some("branch_a"));
}

// a run paused by the previous single-cursor debugger has no per-cursor runtime. it must resume
// with its state intact rather than silently reading as "not paused".
#[test]
fn a_cursor_without_a_runtime_falls_back_to_the_run_frame() {
    let mut parsed = state(serde_json::json!({
        "debug": { "enabled": true, "paused": true, "current_node_id": "verify" },
    }));
    let id = parsed.ensure_cursor("verify");

    let runtime = parsed.cursor_debug(id);
    assert!(runtime.paused);
    assert_eq!(runtime.current_node_id.as_deref(), Some("verify"));
    assert!(parsed.all_cursors_paused());
}

#[test]
fn all_cursors_paused_is_false_for_a_run_with_no_cursors() {
    assert!(!WorkflowRunState::default().all_cursors_paused());
}

#[test]
fn subflow_parent_and_map_child_are_typed_rather_than_bag_keys() {
    let parsed = state(serde_json::json!({
        "subflow_parent": { "run_id": "00000000-0000-0000-0000-00000000000a", "node_id": "fanout" },
        "map_child": { "stop_node": "fanout", "index": 2, "item": { "sku": "x" } },
    }));

    let parent = parsed.subflow_parent.as_ref().expect("subflow_parent");
    assert_eq!(parent.node_id, "fanout");
    let child = parsed.map_child.as_ref().expect("map_child");
    assert_eq!(child.stop_node, "fanout");
    assert_eq!(child.index, 2);
    // both are modeled now, so neither should be left behind in the forward-compat bag.
    assert!(!parsed.extra.contains_key("subflow_parent"));
    assert!(!parsed.extra.contains_key("map_child"));
}

#[test]
fn either_child_marker_makes_a_run_a_child() {
    assert!(!state(serde_json::json!({})).is_child_run());
    assert!(
        state(serde_json::json!({
            "subflow_parent": { "run_id": "00000000-0000-0000-0000-00000000000a", "node_id": "n" },
        }))
        .is_child_run()
    );
    assert!(
        state(serde_json::json!({
            "map_child": { "stop_node": "n", "index": 0, "item": null },
        }))
        .is_child_run()
    );
}

// runs already in flight carry one top-level `event_source_<node_id>` key per subscribed node.
// reading must fold them into `event_sources` so both shapes drive the same code.
#[test]
fn legacy_event_source_keys_fold_into_the_consolidated_map() {
    let parsed = state(serde_json::json!({
        "event_source_watcher": { "pending_event": { "type": "deploy" } },
    }));

    let slot = parsed.event_source("watcher").expect("watcher slot");
    assert_eq!(
        slot.pending_event
            .as_ref()
            .and_then(|event| event.get("type")),
        Some(&Value::from("deploy"))
    );
    assert!(!parsed.extra.contains_key("event_source_watcher"));
}

// a run mid-migration can carry both shapes; the consolidated entry is the newer one and wins.
#[test]
fn a_consolidated_entry_wins_over_a_legacy_key_for_the_same_node() {
    let parsed = state(serde_json::json!({
        "event_sources": { "watcher": { "pending_event": { "type": "new" } } },
        "event_source_watcher": { "pending_event": { "type": "old" } },
    }));

    let slot = parsed.event_source("watcher").expect("watcher slot");
    assert_eq!(
        slot.pending_event
            .as_ref()
            .and_then(|event| event.get("type")),
        Some(&Value::from("new"))
    );
}

#[test]
fn a_delivered_event_round_trips_through_the_state_blob() {
    let mut parsed = WorkflowRunState::default();
    parsed.deliver_event("watcher", serde_json::json!({ "type": "deploy" }).into());

    let reparsed = WorkflowRunState::from_state(&parsed.to_state());
    assert_eq!(
        reparsed
            .event_source("watcher")
            .and_then(|slot| slot.pending_event.as_ref()),
        parsed
            .event_source("watcher")
            .and_then(|slot| slot.pending_event.as_ref())
    );
}

// the frames these tests cover were previously read with `from_wire_value(..).ok()`, so a malformed
// one yielded `None` rather than throwing away everything else the run was tracking.
#[test]
fn a_malformed_frame_does_not_discard_the_rest_of_the_state() {
    let parsed = state(serde_json::json!({
        "map_child": "not an object",
        "watch_fired": true,
        "run_metadata": { "name": "kept" },
    }));

    assert!(parsed.map_child.is_none());
    assert!(parsed.watch_fired);
    assert_eq!(
        parsed
            .run_metadata
            .as_ref()
            .and_then(|meta| meta.get("name")),
        Some(&Value::from("kept"))
    );
}

// unmodeled keys still round-trip, which is what lets a node snapshot ride along in state.
#[test]
fn unmodeled_keys_survive_a_round_trip() {
    let parsed = state(serde_json::json!({ "wait_snapshot": { "deadline_unix": 42 } }));
    let reparsed = WorkflowRunState::from_state(&parsed.to_state());
    assert_eq!(
        reparsed
            .extra
            .get("wait_snapshot")
            .and_then(|snap| snap.get("deadline_unix")),
        Some(&Value::from(42))
    );
}
