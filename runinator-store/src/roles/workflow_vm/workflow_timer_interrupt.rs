#[allow(unused_imports)]
use super::*;

/// One durable periodic timer attached to a running workflow. The schedule is separate from a
/// continuation so a timer survives forks, handler completion, and the continuation it happens to
/// interrupt changing over time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTimerInterrupt {
    pub workflow_run_id: Uuid,
    pub timer_id: String,
    pub interval_seconds: i64,
    pub due_at: DateTime<Utc>,
}
