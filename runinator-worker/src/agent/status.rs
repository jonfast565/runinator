//! the agent's externally visible state: where the lifecycle is in the connect/retry cycle, and the
//! running action counters. both the headless binary and a gui host read the same types, so a
//! degraded agent looks the same in a log line as it does in a status header.

use uuid::Uuid;

use crate::events::{ActionOutcome, WorkerEvent};

/// where the agent lifecycle is in the register/connect/retry cycle. surfaced through
/// [`crate::agent::AgentObserver`] so a degraded agent (service unreachable, broker down, loop
/// crash-looping) is visible without parsing logs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentConnection {
    /// no lifecycle running (never started, or shut down).
    #[default]
    Stopped,
    /// registering the replica with the web service.
    Registering,
    /// building the broker connection and bringing the action loop up.
    Connecting,
    /// the action loop is up and consuming.
    Connected,
    /// the loop exited or the broker failed; backing off before the next attempt.
    Reconnecting { retry_secs: u64 },
    /// the relay rejected this credential; waiting cannot repair it.
    ReenrollmentRequired { reason: String },
}

impl AgentConnection {
    pub fn is_connected(&self) -> bool {
        matches!(self, AgentConnection::Connected)
    }

    /// stable lowercase label, used in logs and (from phase 3) in the status report.
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentConnection::Stopped => "stopped",
            AgentConnection::Registering => "registering",
            AgentConnection::Connecting => "connecting",
            AgentConnection::Connected => "connected",
            AgentConnection::Reconnecting { .. } => "reconnecting",
            AgentConnection::ReenrollmentRequired { .. } => "reenrollment_required",
        }
    }
}

/// a snapshot of the agent lifecycle, republished on every transition.
#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    /// true once the replica is registered and the action loop has been handed its first attempt.
    pub running: bool,
    pub replica_id: Option<Uuid>,
    pub connection: AgentConnection,
    /// how this agent reaches the broker, e.g. `relay via wss://host/ws/desktop-worker`.
    pub broker_connection: Option<String>,
}

/// a single finished action, kept so a host can show what this machine last did.
#[derive(Debug, Clone)]
pub struct CompletedAction {
    pub summary: String,
    pub outcome: ActionOutcome,
    pub duration_ms: i64,
}

/// live action counters folded from the worker event stream. `cpu_percent`/`mem_percent` are filled
/// by the telemetry sampler rather than by events.
#[derive(Debug, Clone, Default)]
pub struct AgentMetrics {
    pub in_flight: u32,
    pub succeeded: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub canceled: u64,
    pub skipped_duplicates: u64,
    pub last_completed: Option<CompletedAction>,
    pub cpu_percent: Option<f32>,
    pub mem_percent: Option<f32>,
}

impl AgentMetrics {
    /// fold one worker-loop event into the counters. saturating throughout: a counter that drifted
    /// (a finish with no matching start after a restart) must never panic the event sink.
    pub fn apply(&mut self, event: &WorkerEvent) {
        match event {
            WorkerEvent::ActionStarted { .. } => {
                self.in_flight = self.in_flight.saturating_add(1);
            }
            WorkerEvent::ActionSkippedDuplicate { .. } => {
                self.skipped_duplicates = self.skipped_duplicates.saturating_add(1);
            }
            WorkerEvent::ActionFinished {
                workflow_run_id,
                provider,
                function,
                node_id,
                outcome,
                duration_ms,
                ..
            } => {
                self.in_flight = self.in_flight.saturating_sub(1);
                match outcome {
                    ActionOutcome::Succeeded => self.succeeded = self.succeeded.saturating_add(1),
                    ActionOutcome::Failed => self.failed = self.failed.saturating_add(1),
                    ActionOutcome::TimedOut => self.timed_out = self.timed_out.saturating_add(1),
                    ActionOutcome::Canceled => self.canceled = self.canceled.saturating_add(1),
                }
                self.last_completed = Some(CompletedAction {
                    summary: format!(
                        "{provider}.{function} ({node_id}, run {})",
                        short_id(workflow_run_id)
                    ),
                    outcome: *outcome,
                    duration_ms: *duration_ms,
                });
            }
            WorkerEvent::ControlReceived { .. } => {}
        }
    }
}

/// first uuid segment; enough to correlate a console line with the run in the command center.
pub fn short_id(id: &Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
