#[allow(unused_imports)]
use super::*;

/// result of atomically joining and attempting to take a mutex queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowMutexClaimResult {
    pub acquired: bool,
    pub holder_overdue: bool,
    pub wake: Option<WorkflowMutexWake>,
}
