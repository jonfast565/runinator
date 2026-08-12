//! the single seam between the shared agent lifecycle and whatever is hosting it. the headless
//! binary leaves every hook at its default (tracing inside the loops already covers it); a gui host
//! implements them to drive a status header, a console, and native notifications.

use crate::agent::status::AgentStatus;
use crate::events::WorkerEvent;

/// host hooks for agent lifecycle activity. implementations must be cheap and non-blocking: hooks
/// are called inline from the lifecycle task and from the worker loops.
pub trait AgentObserver: Send + Sync {
    /// a human-readable lifecycle line (registering, connected, retrying, ...).
    fn on_log(&self, _line: &str) {}

    /// the lifecycle status changed. called on every transition, including the terminal
    /// [`crate::agent::AgentConnection::Stopped`].
    fn on_status(&self, _status: &AgentStatus) {}

    /// a worker loop event. fold it into [`crate::agent::AgentMetrics`] to keep counters.
    fn on_worker_event(&self, _event: &WorkerEvent) {}
}

/// default observer that ignores everything; the headless default.
pub struct NoopObserver;

impl AgentObserver for NoopObserver {}
