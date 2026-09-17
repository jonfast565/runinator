#[allow(unused_imports)]
use super::*;

pub trait AgentObserver: Send + Sync {
    /// a human-readable lifecycle line (registering, connected, retrying, ...).
    fn on_log(&self, _line: &str) {}

    /// the lifecycle status changed. called on every transition, including the terminal
    /// [`crate::agent::AgentConnection::Stopped`].
    fn on_status(&self, _status: &AgentStatus) {}

    /// a worker loop event. fold it into [`crate::agent::AgentMetrics`] to keep counters.
    fn on_worker_event(&self, _event: &WorkerEvent) {}
}
