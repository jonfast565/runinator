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
            ..Self::at(node_id)
        }
    }

    /// is this a debugger "what if" branch rather than a real thread of control?
    pub fn is_speculative(&self) -> bool {
        self.speculative.is_some()
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
    pub fn move_to(&mut self, node_id: impl Into<String>) {
        self.node_id = node_id.into();
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
