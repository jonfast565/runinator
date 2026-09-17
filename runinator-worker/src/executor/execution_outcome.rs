#[allow(unused_imports)]
use super::*;

pub struct ExecutionOutcome {
    pub task_result: ExecutionTaskResult,
    pub execution_result: Option<TaskExecutionResult>,
    pub status: RunStatus,
}
