#[allow(unused_imports)]
use super::*;

/// a durable mutex waiter that should be driven immediately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowMutexWake {
    pub workflow_run_id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub cursor_id: Uuid,
    pub node_id: String,
}
