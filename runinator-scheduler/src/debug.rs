// thin scheduler-side adapters over the typed debug frame. the break decision and frame shape are
// owned by `runinator_models::debug`; this module only exposes the run-level predicates the
// scheduling loop reads and the step-clearing transform it writes.

use runinator_comm::WireError;
use runinator_models::debug::Debuggable;
use runinator_models::value::Value;
use runinator_models::workflows::WorkflowRun;

use crate::nodes::RunState;

pub fn enabled(workflow_run: &WorkflowRun) -> bool {
    RunState::from_run(workflow_run)
        .debug()
        .map(|debug| debug.config.enabled)
        .unwrap_or(false)
}

pub fn paused(workflow_run: &WorkflowRun) -> bool {
    RunState::from_run(workflow_run)
        .debug()
        .map(|debug| debug.runtime.paused)
        .unwrap_or(false)
}

pub fn step_requested(workflow_run: &WorkflowRun) -> bool {
    RunState::from_run(workflow_run)
        .debug()
        .map(|debug| debug.runtime.step_requested)
        .unwrap_or(false)
}

pub fn one_shot_breakpoint(workflow_run: &WorkflowRun) -> Option<String> {
    RunState::from_run(workflow_run)
        .debug()
        .and_then(|debug| debug.runtime.one_shot_breakpoint.clone())
}

/// returns true when this node should pause the run based on mode and breakpoints.
pub fn should_break_at(workflow_run: &WorkflowRun, node_id: &str) -> bool {
    workflow_run.should_break_at(&node_id.to_string())
}

/// after consuming a step or one-shot breakpoint, clear those flags but
/// preserve user-owned fields like mode and breakpoints.
pub fn state_with_step_cleared(state: Value) -> Result<Value, WireError> {
    let mut run_state = RunState::from_value(&state);
    if run_state.debug().is_some() {
        let debug = run_state.debug_mut();
        debug.runtime.paused = false;
        debug.runtime.step_requested = false;
        debug.runtime.one_shot_breakpoint = None;
    }
    run_state.into_value()
}
