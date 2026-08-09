// a run's position in its workflow graph, and the frames scoped to that position.
//
// a run is a track and a cursor is a place on it. the run carries its cursors in
// `WorkflowRunState`; `workflow_runs.active_node_id` mirrors the primary one so the wire and ui
// contract is unchanged.
//
// frames that belong to one thread of control (a loop iteration, a try phase) live on the cursor
// rather than on the run, because two cursors running concurrently would otherwise share — and
// corrupt — a single frame.

use std::collections::BTreeSet;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::interrupt::InterruptFrame;
use crate::value::Value;
use crate::workflow_state::{DebugRuntime, LoopFrame, TryFrame};
use crate::workflows::WorkflowRun;

/// marks a cursor as a debugger-spawned "what if" branch rather than a real thread of control.
///
/// a speculative cursor walks the same graph beside the real ones, but it must not be able to change
/// what the run means: it never satisfies a join, never moves the run's status, and shadows any node
/// whose processing would escape the run unless that node is explicitly armed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeculativeFrame {
    /// the cursor this one was forked from, so a nested fork drains as a unit.
    pub forked_from_cursor: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
    /// nodes opted in to real dispatch; every other external-effect node shadows.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub armed_nodes: BTreeSet<String>,
    /// merge-patch overlaid on this cursor's resolved context, for "what if this value differed".
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub context_patch: Value,
}

/// a position on a run's track: the node about to be processed, plus the control-flow frames
/// belonging to that thread of control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunCursor {
    /// stable identity, so a ready-queue row can name the cursor it should drive.
    #[serde(default = "Uuid::now_v7")]
    pub id: Uuid,
    node_id: String,
    /// loop-iteration bookkeeping for a loop body this cursor is inside.
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub loop_frame: Option<LoopFrame>,
    /// try/catch/finally phase bookkeeping for a try region this cursor is inside.
    #[serde(rename = "try", default, skip_serializing_if = "Option::is_none")]
    pub try_frame: Option<TryFrame>,
    /// the fan-out node that forked this cursor, for cursors created by `parallel`/`race`. `None`
    /// for the run's original cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_by: Option<String>,
    /// set when this cursor is a debugger "what if" branch rather than a real thread of control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speculative: Option<SpeculativeFrame>,
    /// this thread of control's debugger state. run-scoped config (breakpoints, mode) stays on
    /// `DebugFrame`; what is per-thread — paused, step_requested, the inspection snapshot — lives
    /// here, because stepping one branch must not step its siblings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugRuntime>,
    /// the output of the last node this cursor finished, for the debugger's diff pane. run-wide
    /// "most recently finished output" is the wrong answer once a run has fan-out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_output: Option<Value>,
    /// set when this cursor is an interrupt handler rather than an ordinary thread of control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupt: Option<InterruptFrame>,
    /// the handler cursor that froze this one. a suspended cursor is never driven, and never moved
    /// by anything but the handler returning control to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended_by: Option<Uuid>,
    /// interrupts already raised at this position, keyed by `<source>:<node_run_id>`.
    ///
    /// this is what stops a plain `resume` from re-raising forever: after resuming, the condition
    /// that raised the interrupt (an elapsed wait deadline, say) is still true, so without a record
    /// the source would match again on the very next drive. cleared by [`RunCursor::move_to`],
    /// because a different position is a different question.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub handled: BTreeSet<String>,
    /// seconds this cursor spent frozen behind an interrupt while standing where it stands now.
    ///
    /// a parked node's deadline is measured from its node run's `created_at`, so without this a
    /// ten-minute handler would silently eat ten minutes of an approval's window and the park would
    /// time out the moment control came back. credited by the timeout checks and cleared by
    /// [`RunCursor::move_to`], because the debt belongs to the position that incurred it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub suspended_seconds: i64,
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

impl RunCursor {
    /// a fresh cursor pointing at `node_id`.
    pub fn at(node_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            node_id: node_id.into(),
            loop_frame: None,
            try_frame: None,
            forked_by: None,
            speculative: None,
            debug: None,
            last_output: None,
            interrupt: None,
            suspended_by: None,
            handled: BTreeSet::new(),
            suspended_seconds: 0,
        }
    }

    /// a branch cursor forked from `forked_by`, entering the branch at `node_id`.
    pub fn forked(node_id: impl Into<String>, forked_by: impl Into<String>) -> Self {
        Self {
            forked_by: Some(forked_by.into()),
            ..Self::at(node_id)
        }
    }

    /// a speculative branch of `parent`, entering at `node_id`.
    ///
    /// the fork inherits the parent's control-flow frames, so forking inside a loop iteration or a
    /// try phase explores from where the parent actually stands rather than from a clean slate.
    pub fn speculative_from(
        parent: &RunCursor,
        node_id: impl Into<String>,
        label: Option<String>,
        context_patch: Value,
    ) -> Self {
        Self {
            speculative: Some(SpeculativeFrame {
                forked_from_cursor: parent.id,
                label,
                created_at: Utc::now(),
                armed_nodes: BTreeSet::new(),
                context_patch,
            }),
            loop_frame: parent.loop_frame.clone(),
            try_frame: parent.try_frame.clone(),
            // the interrupt fields are deliberately *not* inherited: a fork of a suspended cursor
            // would otherwise be born frozen, and a fork of a handler would claim to be one too.
            // `Self::at` leaves all three empty, which is the answer we want.
            ..Self::at(node_id)
        }
    }

    /// a handler cursor entering `node_id`, the region entry of the interrupt it answers.
    pub fn interrupt_handler(node_id: impl Into<String>, frame: InterruptFrame) -> Self {
        Self {
            interrupt: Some(frame),
            ..Self::at(node_id)
        }
    }

    /// is this a debugger "what if" branch rather than a real thread of control?
    pub fn is_speculative(&self) -> bool {
        self.speculative.is_some()
    }

    /// is this cursor running an interrupt handler region rather than the run's own flow?
    ///
    /// a handler is a side-channel: it never satisfies a join, never moves the run's status, and
    /// retires without ending the run — the same carve-outs a speculative cursor gets, for the
    /// same reason. it must not be able to decide what the run means.
    pub fn is_interrupt_handler(&self) -> bool {
        self.interrupt.is_some()
    }

    /// is this cursor frozen while an interrupt handler runs? a suspended cursor is not driven and
    /// cannot be moved; only the handler returning control releases it.
    pub fn is_suspended(&self) -> bool {
        self.suspended_by.is_some()
    }

    /// has this interrupt already been raised at this position?
    pub fn has_handled(&self, key: &str) -> bool {
        self.handled.contains(key)
    }

    /// record that an interrupt fired here, so it does not immediately re-raise after a `resume`.
    pub fn mark_handled(&mut self, key: impl Into<String>) {
        self.handled.insert(key.into());
    }

    /// how long a deadline measured at this position should be extended by, to discount the time
    /// this thread spent frozen rather than waiting for the thing it is actually waiting on.
    pub fn suspension_credit(&self) -> chrono::Duration {
        chrono::Duration::seconds(self.suspended_seconds.max(0))
    }

    /// may this cursor dispatch `node_id` for real, rather than shadowing it? always true for a real
    /// cursor; true for a speculative one only where the operator explicitly armed the node.
    pub fn is_armed_for(&self, node_id: &str) -> bool {
        match &self.speculative {
            Some(frame) => frame.armed_nodes.contains(node_id),
            None => true,
        }
    }

    /// the run's persisted position, or `None` for a run that has not been placed yet.
    ///
    /// use this when the absence of a position is meaningful; use [`RunCursor::resolve`] when an
    /// unplaced run should be treated as sitting on its start node.
    pub fn of(run: &WorkflowRun) -> Option<Self> {
        run.active_node_id.as_deref().map(Self::at)
    }

    /// the run's position, falling back to `start` when it has not been placed yet.
    ///
    /// this is the rule the reducer drives on: a freshly created run carries no `active_node_id`
    /// and enters its graph at the start node.
    pub fn resolve(run: &WorkflowRun, start: &str) -> Self {
        Self::of(run).unwrap_or_else(|| Self::at(start))
    }

    /// the node id this cursor points at.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// is this cursor sitting on `node_id`?
    pub fn is_at(&self, node_id: &str) -> bool {
        self.node_id == node_id
    }

    /// move this cursor to `node_id`, leaving its frames alone.
    ///
    /// the fired-interrupt record is dropped, because it is keyed to the position being left: an
    /// interrupt that already answered for the previous node has nothing to say about the next one.
    pub fn move_to(&mut self, node_id: impl Into<String>) {
        let node_id = node_id.into();
        if node_id != self.node_id {
            self.handled.clear();
            self.suspended_seconds = 0;
        }
        self.node_id = node_id;
    }

    /// clear the frames belonging to this thread of control, for a loop body re-entering cleanly.
    /// run-scoped state is untouched, which is the point: only this cursor resets.
    pub fn clear_frames(&mut self) {
        self.loop_frame = None;
        self.try_frame = None;
    }

    /// consume the cursor for its node id, for the store calls that persist a position.
    pub fn into_node_id(self) -> String {
        self.node_id
    }
}

impl fmt::Display for RunCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.node_id)
    }
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod cursor_tests;
