// typed manipulation of a workflow run's `state`.
//
// control-flow nodes (loop, parallel, map, race, try) keep bookkeeping in named frames inside
// `workflow_run.state`. `RunState` wraps the typed `WorkflowRunState` container so handlers
// manipulate frames by intent instead of hand-rolling json. it performs pure value transforms only;
// the caller persists the rebuilt value through `WorkflowSchedulerApi::update_workflow_run`,
// converting to the dynamic carrier via `WireCodec::to_wire_value`.
//
// predicates that read sibling node-run history (`join_satisfied`, `race_winner`,
// `append_completed_map_item`) stay as functions here since they read run history, not the blob.

use runinator_comm::{WireCodec, WireError};
use runinator_models::value::Value;
use runinator_models::workflow_state::{
    ControlFrame, DebugFrame, LoopFrame, MapFrame, ParallelFrame, RaceFrame, TryFrame,
    WorkflowRunState,
};
use runinator_models::workflows::WorkflowRun;

// the node-run-history predicates now live in `runinator-workflows` so both the scheduler and the
// web-service reducer share one copy. re-export them here so existing handler imports keep working.
pub use runinator_workflows::{
    append_completed_map_item, branch_policy_name, join_satisfied, latest_status, race_winner,
};

/// typed read/builder over a workflow run's `state` container.
#[derive(Clone, Default)]
pub struct RunState {
    inner: WorkflowRunState,
}

impl RunState {
    /// borrow the run's state into a typed builder. malformed/non-object state collapses to empty.
    pub fn from_run(run: &WorkflowRun) -> Self {
        Self::from_value(&run.state)
    }

    pub fn from_value(value: &Value) -> Self {
        Self {
            inner: WorkflowRunState::from_wire_value(value).unwrap_or_default(),
        }
    }

    /// consume the builder back into a wire value for persistence.
    pub fn into_value(self) -> Result<Value, WireError> {
        self.inner.to_wire_value()
    }

    // named frame access (loop/parallel/map/race/try/control/debug).

    pub fn loop_frame(&self) -> Option<&LoopFrame> {
        self.inner.loop_frame.as_ref()
    }

    pub fn set_loop(&mut self, frame: LoopFrame) -> &mut Self {
        self.inner.loop_frame = Some(frame);
        self
    }

    pub fn clear_loop(&mut self) -> &mut Self {
        self.inner.loop_frame = None;
        self
    }

    pub fn set_parallel(&mut self, frame: ParallelFrame) -> &mut Self {
        self.inner.parallel = Some(frame);
        self
    }

    pub fn map(&self) -> Option<&MapFrame> {
        self.inner.map.as_ref()
    }

    pub fn set_map(&mut self, frame: MapFrame) -> &mut Self {
        self.inner.map = Some(frame);
        self
    }

    pub fn set_race(&mut self, frame: RaceFrame) -> &mut Self {
        self.inner.race = Some(frame);
        self
    }

    /// true when the race frame belongs to `node_id`.
    pub fn race_owned_by(&self, node_id: &str) -> bool {
        self.inner
            .race
            .as_ref()
            .is_some_and(|frame| frame.node_id == node_id)
    }

    pub fn try_frame(&self) -> Option<&TryFrame> {
        self.inner.try_frame.as_ref()
    }

    pub fn set_try(&mut self, frame: TryFrame) -> &mut Self {
        self.inner.try_frame = Some(frame);
        self
    }

    pub fn control(&self) -> Option<&ControlFrame> {
        self.inner.control.as_ref()
    }

    /// ensure a control frame exists without clobbering an existing one.
    pub fn ensure_control(&mut self) -> &mut Self {
        self.inner.control.get_or_insert_with(ControlFrame::default);
        self
    }

    pub fn debug(&self) -> Option<&DebugFrame> {
        self.inner.debug.as_ref()
    }

    /// borrow the debug frame mutably, creating a default one if absent.
    pub fn debug_mut(&mut self) -> &mut DebugFrame {
        self.inner.debug.get_or_insert_with(DebugFrame::default)
    }

    pub fn set_debug(&mut self, frame: DebugFrame) -> &mut Self {
        self.inner.debug = Some(frame);
        self
    }

    pub fn run_metadata(&self) -> Option<&Value> {
        self.inner.run_metadata.as_ref()
    }

    pub fn set_run_metadata(&mut self, value: Value) -> &mut Self {
        self.inner.run_metadata = Some(value);
        self
    }

    // queue ops used by parallel/race fan-out.

    /// pop the head of the parallel frame's `remaining` queue, leaving the rest in place.
    pub fn pop_parallel_remaining(&mut self) -> Option<String> {
        self.inner.parallel.as_mut()?.pop_remaining()
    }

    /// pop the head of the race frame's `remaining` queue, leaving the rest in place.
    pub fn pop_race_remaining(&mut self) -> Option<String> {
        self.inner.race.as_mut()?.pop_remaining()
    }
}
