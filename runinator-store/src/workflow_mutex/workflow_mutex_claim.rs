#[allow(unused_imports)]
use super::*;

/// one cursor's request to enter a named workflow mutex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowMutexClaim {
    pub name: String,
    pub workflow_run_id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub cursor_id: Uuid,
    pub node_id: String,
    pub hold_deadline_unix: Option<i64>,
    pub enqueued_at_unix: i64,
}
