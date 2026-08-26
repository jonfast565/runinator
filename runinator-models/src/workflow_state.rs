// typed representations of workflow execution state and node-run state/output payloads.
//
// the scheduler manipulates these as structs and converts to/from the dynamic `Value` carriers
// (normalized execution tables, workflow_node_run.state, output_json) at persistence boundaries via
// `runinator_comm::WireCodec`. the web service still owns the same wire shapes, so these structs
// mirror the keys it reads and writes. unmodeled keys round-trip through `#[serde(flatten)]` bags.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cursor::RunCursor;
use crate::interrupt::PendingInterrupt;
use crate::value::{Map, Value};

pub use crate::workflow_coordination::*;
pub use crate::workflow_frames::*;
pub use crate::workflow_node_states::*;
pub use crate::workflow_outputs::*;

// deserialize a frame tolerantly: a malformed payload becomes `None` rather than failing the parse
// of the whole state blob. these frames were previously read out of the untyped bag with
// `from_wire_value(..).ok()`, and that tolerance is what stops one bad frame from discarding a
// run's entire state.
fn lenient_frame<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let raw = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(raw).ok())
}

/// typed workflow execution aggregate: named control-flow frames plus user-authored values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowExecutionState {
    /// where the run is on its track. one entry for a linear run; `parallel`/`race` fan out more.
    /// empty for a run that has not been placed yet, which the reducer seeds on its first drive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cursors: Vec<RunCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<ControlFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<MapFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<CompensationFrame>,
    /// set on a child run (subflow or map item) so reaching a terminal can wake the parent node
    /// that started it.
    #[serde(
        default,
        deserialize_with = "lenient_frame",
        skip_serializing_if = "Option::is_none"
    )]
    pub subflow_parent: Option<SubflowParent>,
    /// set on a map fan-out child: the item it is bound to and where its body must stop.
    #[serde(
        default,
        deserialize_with = "lenient_frame",
        skip_serializing_if = "Option::is_none"
    )]
    pub map_child: Option<MapChildState>,
    /// Per-node event-source delivery slots, keyed by node id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub event_sources: BTreeMap<String, EventSourceEntry>,
    /// dynamic per-run metadata bag accumulated by config nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_metadata: Option<Value>,
    /// set once a workflow-level `watch` guard has fired, so it redirects to its handler at most once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub watch_fired: bool,
    /// interrupts asked for from outside the run, waiting for the next drive of their target thread
    /// to raise or refuse them. empty for every run that is never interrupted from outside.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_interrupts: Vec<PendingInterrupt>,
    /// preserves any keys not modeled above (e.g. wait/subflow node snapshots mirrored into state).
    #[serde(flatten)]
    pub extra: Map,
}

impl WorkflowExecutionState {
    /// parse a run's `state` blob into the typed container. malformed state collapses to empty.
    pub fn from_state(value: &Value) -> Self {
        serde_json::from_value(value.clone().into()).unwrap_or_default()
    }

    /// the cursor with this id, if the run still holds it.
    pub fn cursor(&self, id: Uuid) -> Option<&RunCursor> {
        self.cursors.iter().find(|cursor| cursor.id == id)
    }

    /// mutable access to the cursor with this id.
    pub fn cursor_mut(&mut self, id: Uuid) -> Option<&mut RunCursor> {
        self.cursors.iter_mut().find(|cursor| cursor.id == id)
    }

    /// the first cursor sitting on `node_id`. used to resolve a ready-queue row enqueued before
    /// cursors carried ids.
    pub fn cursor_at(&self, node_id: &str) -> Option<&RunCursor> {
        self.cursors.iter().find(|cursor| cursor.is_at(node_id))
    }

    /// the cursor mirrored into `workflow_runs.active_node_id`, and the one single-threaded
    /// consumers (the debugger, the run detail UI) follow.
    pub fn primary_cursor(&self) -> Option<&RunCursor> {
        self.cursors.first()
    }

    /// place a run that has no cursors yet, returning the seeded cursor's id. a no-op returning the
    /// existing primary when the run is already placed.
    pub fn ensure_cursor(&mut self, node_id: &str) -> Uuid {
        if let Some(cursor) = self.cursors.first() {
            return cursor.id;
        }
        let cursor = RunCursor::at(node_id);
        let id = cursor.id;
        self.cursors.push(cursor);
        id
    }

    /// fork a branch cursor off `parent`, entering `node_id`, attributed to the fan-out node
    /// `forked_by`.
    ///
    /// the branch inherits the parent's control-flow frames; `parent` is named rather than inferred
    /// because the forking cursor retires immediately afterwards, so this is the only moment its
    /// position can be carried across.
    pub fn fork_cursor(&mut self, parent: Uuid, node_id: &str, forked_by: &str) -> Uuid {
        let cursor = match self.cursor(parent) {
            Some(parent) => RunCursor::forked_from(parent, node_id, forked_by),
            None => RunCursor::forked(node_id, forked_by),
        };
        let id = cursor.id;
        self.cursors.push(cursor);
        id
    }

    /// drop a cursor that has reached the end of its thread of control. returns whether it was
    /// still there, so a caller can tell a first retirement from a repeat.
    /// retiring a cursor also retires the interrupt handler attached to it. a handler exists only
    /// to hand control back to the thread it suspended, so once that thread is gone the handler has
    /// nowhere to return to and would otherwise keep the run open forever. nesting is not
    /// supported, so this cascade is exactly one level deep.
    pub fn retire_cursor(&mut self, id: Uuid) -> bool {
        let before = self.cursors.len();
        self.cursors.retain(|cursor| {
            cursor.id != id
                && cursor
                    .interrupt
                    .as_ref()
                    .is_none_or(|frame| frame.interrupted_cursor != id)
        });
        self.cursors.len() != before
    }

    /// the oldest interrupt request this cursor may take, if any.
    ///
    /// oldest-first so a burst of requests is served in the order it was made rather than the order
    /// the run happens to drive.
    pub fn pending_interrupt_for(&self, cursor_id: Uuid) -> Option<&PendingInterrupt> {
        self.pending_interrupts
            .iter()
            .filter(|request| request.targets(cursor_id))
            .min_by_key(|request| request.requested_at)
    }

    /// drop a request once a drive has decided about it. returns whether it was still there, so a
    /// caller can tell a first decision from a duplicated drive re-deciding.
    pub fn take_pending_interrupt(&mut self, id: Uuid) -> bool {
        let before = self.pending_interrupts.len();
        self.pending_interrupts.retain(|request| request.id != id);
        self.pending_interrupts.len() != before
    }

    /// the handler cursor currently suspending `id`, if one is running.
    pub fn handler_for(&self, id: Uuid) -> Option<&RunCursor> {
        self.cursors.iter().find(|cursor| {
            cursor
                .interrupt
                .as_ref()
                .is_some_and(|frame| frame.interrupted_cursor == id)
        })
    }

    /// every *real* cursor forked by `forked_by`, for a join deciding whether its branches have all
    /// arrived and for a race retiring its losers.
    ///
    /// speculative cursors are excluded: a debugger "what if" branch must not be able to make a
    /// join think a real branch arrived, nor a race think it has already fanned out.
    pub fn cursors_forked_by(&self, forked_by: &str) -> impl Iterator<Item = &RunCursor> {
        self.cursors.iter().filter(move |cursor| {
            cursor.speculative.is_none()
                && cursor
                    .forked_by
                    .as_deref()
                    .is_some_and(|origin| origin == forked_by)
        })
    }

    /// the real threads of control — everything the run's completion actually depends on.
    ///
    /// a *suspended* cursor is deliberately included: it is a real thread that will resume, so it
    /// must keep the run alive in the reducer's terminal accounting. an interrupt handler is
    /// included too, for the same reason — the run is not finished while one is executing.
    pub fn real_cursors(&self) -> impl Iterator<Item = &RunCursor> {
        self.cursors
            .iter()
            .filter(|cursor| !cursor.is_speculative())
    }

    /// the cursors a `join` may count as arrived branches, and a `race` may retire as losers.
    ///
    /// this excludes interrupt handlers on the same grounds as speculative cursors: a handler is a
    /// side-channel, so counting one as a sibling would let a genuinely-alone branch conclude it
    /// has company and retire itself into a stall.
    pub fn joinable_cursors(&self) -> impl Iterator<Item = &RunCursor> {
        self.cursors
            .iter()
            .filter(|cursor| !cursor.is_speculative() && !cursor.is_interrupt_handler())
    }

    /// is the cursor with this id a debugger "what if" branch?
    pub fn is_speculative(&self, id: Uuid) -> bool {
        self.cursor(id).is_some_and(RunCursor::is_speculative)
    }

    /// `root` plus every speculative cursor transitively forked from it, so an abandoned or failed
    /// fork drains as a unit instead of leaving orphaned children behind.
    pub fn speculative_subtree(&self, root: Uuid) -> Vec<Uuid> {
        let mut found = vec![root];
        let mut index = 0;
        while index < found.len() {
            let parent = found[index];
            index += 1;
            for cursor in &self.cursors {
                let forked_from = cursor
                    .speculative
                    .as_ref()
                    .map(|frame| frame.forked_from_cursor);
                if forked_from == Some(parent) && !found.contains(&cursor.id) {
                    found.push(cursor.id);
                }
            }
        }
        found
    }

    /// `id` plus every speculative cursor it was forked from, walking up to the first real one.
    ///
    /// this is a fork's *history*: a branch forked from another continues that other's exploration,
    /// so the parent's recorded work is part of this branch's past. the descendants in
    /// [`Self::speculative_subtree`] are the opposite relation — divergent continuations, whose work
    /// this branch must not see — which is why draining and visibility use different walks.
    pub fn speculative_ancestry(&self, id: Uuid) -> Vec<Uuid> {
        let mut chain = vec![id];
        let mut current = id;
        // bounded by the cursor count: each step moves to a strictly different cursor, and a cursor
        // already in the chain stops the walk, so a corrupted cycle cannot spin here.
        while let Some(parent) = self
            .cursor(current)
            .and_then(|cursor| cursor.speculative.as_ref())
            .map(|frame| frame.forked_from_cursor)
        {
            if chain.contains(&parent) {
                break;
            }
            chain.push(parent);
            current = parent;
        }
        chain
    }

    /// fork a speculative branch of `from`, entering at `node_id`. returns the new cursor's id, or
    /// `None` when `from` has already been retired.
    pub fn fork_speculative(
        &mut self,
        from: Uuid,
        node_id: &str,
        label: Option<String>,
        context_patch: Value,
    ) -> Option<Uuid> {
        let parent = self.cursor(from)?;
        let cursor = RunCursor::speculative_from(parent, node_id, label, context_patch);
        let id = cursor.id;
        self.cursors.push(cursor);
        Some(id)
    }

    /// the debugger runtime governing one cursor.
    ///
    /// falls back to the run-scoped frame only while *no* cursor carries a runtime of its own —
    /// which is exactly a run persisted before per-cursor debug state, so it resumes intact. once
    /// any cursor has been written, the flat frame is the primary's mirror rather than the run's
    /// state, and a sibling without a runtime is simply not under the debugger.
    pub fn cursor_debug(&self, id: Uuid) -> DebugRuntime {
        if let Some(runtime) = self.cursor(id).and_then(|cursor| cursor.debug.clone()) {
            return runtime;
        }
        if self.cursors.iter().any(|cursor| cursor.debug.is_some()) {
            return DebugRuntime::default();
        }
        self.debug
            .as_ref()
            .map(|frame| frame.runtime.clone())
            .unwrap_or_default()
    }

    /// write one cursor's debugger runtime, then refresh the run-scoped mirror.
    pub fn set_cursor_debug(&mut self, id: Uuid, runtime: DebugRuntime) {
        if let Some(cursor) = self.cursor_mut(id) {
            cursor.debug = Some(runtime);
        }
        self.mirror_primary_debug();
    }

    /// copy the primary cursor's runtime into the flat `debug` object.
    ///
    /// the frame is the wire contract single-position clients read, so it has to follow whichever
    /// cursor `active_node_id` is reporting.
    pub fn mirror_primary_debug(&mut self) {
        let Some(runtime) = self
            .primary_cursor()
            .and_then(|cursor| cursor.debug.clone())
        else {
            return;
        };
        if let Some(frame) = self.debug.as_mut() {
            frame.runtime = runtime;
        }
    }

    /// is every live cursor parked under the debugger?
    ///
    /// this is the whole condition for the run itself being `DebugPaused`: one branch stopping at a
    /// breakpoint leaves the run `Running`, because its siblings are still executing.
    /// suspended cursors are skipped: one frozen behind an interrupt is not going to reach a
    /// breakpoint on its own, so counting it as unpaused would keep a run out of `DebugPaused`
    /// while the handler itself sits at one.
    pub fn all_cursors_paused(&self) -> bool {
        let mut live = self
            .cursors
            .iter()
            .filter(|cursor| !cursor.is_suspended())
            .peekable();
        live.peek().is_some() && live.all(|cursor| self.cursor_debug(cursor.id).paused)
    }

    /// is this run a child of another run's node (a subflow child or a map item)? child runs must
    /// not fan out further chained workflows or pipelines — only top-level runs chain.
    pub fn is_child_run(&self) -> bool {
        self.subflow_parent.is_some() || self.map_child.is_some()
    }

    /// the delivery slot an event_source node reads, if one has been stamped.
    pub fn event_source(&self, node_id: &str) -> Option<&EventSourceEntry> {
        self.event_sources.get(node_id)
    }

    /// stamp an inbound event for `node_id`, replacing any undelivered one.
    pub fn deliver_event(&mut self, node_id: &str, event: Value) {
        self.event_sources
            .entry(node_id.to_string())
            .or_default()
            .pending_event = Some(event);
    }

    /// Serialize the typed aggregate for JSON transport snapshots.
    pub fn to_state(&self) -> Value {
        serde_json::to_value(self)
            .map(Value::from)
            .unwrap_or(Value::Null)
    }
}

#[cfg(test)]
#[path = "workflow_state_tests.rs"]
mod workflow_state_tests;

// node-run `state` snapshots (workflow_node_run.state).

// node output payloads (serialized into the output_json carrier).
