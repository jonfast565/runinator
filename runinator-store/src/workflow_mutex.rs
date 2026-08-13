//! normalized workflow mutex persistence values exchanged with the reducer.

use uuid::Uuid;

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

/// a durable mutex waiter that should be driven immediately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowMutexWake {
    pub workflow_run_id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub cursor_id: Uuid,
    pub node_id: String,
}

/// result of atomically joining and attempting to take a mutex queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowMutexClaimResult {
    pub acquired: bool,
    pub holder_overdue: bool,
    pub wake: Option<WorkflowMutexWake>,
}
