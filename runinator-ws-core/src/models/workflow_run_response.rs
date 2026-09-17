#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize)]
pub struct WorkflowRunResponse {
    pub run: WorkflowRun,
    pub nodes: Vec<WorkflowNodeRun>,
    pub execution_state: runinator_models::workflow_state::WorkflowExecutionState,
}

impl WorkflowRunResponse {
    pub fn new(run: WorkflowRun, nodes: Vec<WorkflowNodeRun>) -> Self {
        let execution_state = run.execution_state.clone();
        Self {
            run,
            nodes,
            execution_state,
        }
    }
}
