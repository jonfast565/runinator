//! observer hook for worker-loop activity. an embedding host (e.g. the desktop agent's status
//! console) implements [`WorkerEventSink`] to surface what the loop is processing as it happens;
//! the standalone binary uses [`NoopEventSink`] since tracing already covers it there.

use runinator_comm::ControlKind;
use uuid::Uuid;

/// terminal outcome of one action execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    Succeeded,
    Failed,
    TimedOut,
    Canceled,
}

impl ActionOutcome {
    /// stable lowercase label; shared with the worker metrics outcome dimension.
    pub fn as_str(self) -> &'static str {
        match self {
            ActionOutcome::Succeeded => "succeeded",
            ActionOutcome::Failed => "failed",
            ActionOutcome::TimedOut => "timed_out",
            ActionOutcome::Canceled => "canceled",
        }
    }
}

/// a notable moment in the worker's action/control loops, emitted as it happens.
#[derive(Debug, Clone)]
pub enum WorkerEvent {
    /// an action delivery passed the duplicate-lease check and is about to execute.
    ActionStarted {
        workflow_run_id: Uuid,
        node_id: String,
        node_run_id: Uuid,
        provider: String,
        function: String,
        attempt: i64,
    },
    /// a redelivered duplicate was dropped because another executor holds the lease.
    ActionSkippedDuplicate { node_run_id: Uuid },
    /// an action reached a terminal outcome. `duration_ms` is 0 when it failed before execution
    /// (e.g. secret resolution).
    ActionFinished {
        workflow_run_id: Uuid,
        node_id: String,
        node_run_id: Uuid,
        provider: String,
        function: String,
        outcome: ActionOutcome,
        duration_ms: i64,
        message: Option<String>,
    },
    /// a control command (cancel/pause/resume) was received for a run.
    ControlReceived {
        kind: ControlKind,
        workflow_run_id: Uuid,
    },
}

/// observer for [`WorkerEvent`]s. implementations must be cheap and non-blocking: events are
/// emitted inline from the worker loops.
pub trait WorkerEventSink: Send + Sync {
    fn handle(&self, event: WorkerEvent);
}

/// default sink that ignores every event.
pub struct NoopEventSink;

impl WorkerEventSink for NoopEventSink {
    fn handle(&self, _event: WorkerEvent) {}
}

// let embedding hosts pass a plain closure instead of defining a sink type.
impl<F> WorkerEventSink for F
where
    F: Fn(WorkerEvent) + Send + Sync,
{
    fn handle(&self, event: WorkerEvent) {
        self(event)
    }
}
