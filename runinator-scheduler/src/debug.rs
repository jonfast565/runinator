// centralized helpers for the `state.debug` frame.
// every read defaults so legacy runs without new fields still behave.

use runinator_comm::WireError;
use runinator_models::workflows::WorkflowRun;
use serde_json::Value;

use crate::nodes::RunState;

pub const MODE_STEP_ALL: &str = "step_all";
pub const MODE_BREAKPOINTS: &str = "breakpoints";

pub fn enabled(workflow_run: &WorkflowRun) -> bool {
    RunState::from_run(workflow_run)
        .debug()
        .map(|debug| debug.enabled)
        .unwrap_or(false)
}

pub fn paused(workflow_run: &WorkflowRun) -> bool {
    RunState::from_run(workflow_run)
        .debug()
        .map(|debug| debug.paused)
        .unwrap_or(false)
}

pub fn step_requested(workflow_run: &WorkflowRun) -> bool {
    RunState::from_run(workflow_run)
        .debug()
        .map(|debug| debug.step_requested)
        .unwrap_or(false)
}

pub fn mode(workflow_run: &WorkflowRun) -> &'static str {
    let mode = RunState::from_run(workflow_run)
        .debug()
        .and_then(|debug| debug.mode.clone());
    match mode.as_deref() {
        Some(MODE_BREAKPOINTS) => MODE_BREAKPOINTS,
        _ => MODE_STEP_ALL,
    }
}

pub fn breakpoints(workflow_run: &WorkflowRun) -> Vec<String> {
    RunState::from_run(workflow_run)
        .debug()
        .map(|debug| debug.breakpoints.clone())
        .unwrap_or_default()
}

pub fn one_shot_breakpoint(workflow_run: &WorkflowRun) -> Option<String> {
    RunState::from_run(workflow_run)
        .debug()
        .and_then(|debug| debug.one_shot_breakpoint.clone())
}

/// returns true when this node should pause the run based on mode and breakpoints.
pub fn should_break_at(workflow_run: &WorkflowRun, node_id: &str) -> bool {
    if mode(workflow_run) == MODE_STEP_ALL {
        return true;
    }
    if breakpoints(workflow_run).iter().any(|id| id == node_id) {
        return true;
    }
    matches!(one_shot_breakpoint(workflow_run), Some(id) if id == node_id)
}

/// after consuming a step or one-shot breakpoint, clear those flags but
/// preserve user-owned fields like mode and breakpoints.
pub fn state_with_step_cleared(state: Value) -> Result<Value, WireError> {
    let mut run_state = RunState::from_value(&state);
    if run_state.debug().is_some() {
        let debug = run_state.debug_mut();
        debug.paused = false;
        debug.step_requested = false;
        debug.one_shot_breakpoint = None;
    }
    run_state.into_value()
}
