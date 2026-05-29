// centralized helpers for the `state.control` frame.
// every read defaults so legacy runs without new fields still behave.

use runinator_models::workflows::WorkflowRun;

use crate::nodes::RunState;

pub fn pause_requested(workflow_run: &WorkflowRun) -> bool {
    RunState::from_run(workflow_run)
        .control()
        .map(|control| control.pause_requested)
        .unwrap_or(false)
}
