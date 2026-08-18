//! the agent's externally visible state: where the lifecycle is in the connect/retry cycle, and the
//! running action counters. both the headless binary and a gui host read the same types, so a
//! degraded agent looks the same in a log line as it does in a status header.

use std::time::Instant;

use chrono::{DateTime, Utc};
use runinator_models::replicas::{AgentConnectionState, AgentStatusReport};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::agent::config::AgentRuntimeConfig;
use crate::agent::outbox::ResultOutbox;
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
    /// the loop exited or the broker failed; backing off before the next attempt. `attempt` is
    /// 1-based and counts *consecutive* failures, so it resets once an attempt stays up.
    Reconnecting {
        retry_secs: u64,
        attempt: u32,
        /// the budget this attempt counts against; `None` when the agent retries indefinitely.
        max_attempts: Option<u32>,
    },
    /// the reconnect budget is spent: the agent gave up and stopped rather than retrying forever
    /// against a service or broker that is not coming back. terminal — only a fresh start clears it.
    Disconnected { attempts: u32, reason: String },
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
            AgentConnection::Disconnected { .. } => "disconnected",
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
    pub metrics: AgentMetrics,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
}

/// immutable facts combined with each live status snapshot to build the wire report.
pub struct AgentReportContext {
    started_at: Instant,
    broker_mode: String,
    broker_endpoint: String,
    agent_version: Option<String>,
    config_hash: String,
    provider_count: usize,
    labels: std::collections::BTreeMap<String, String>,
    stale_after_seconds: u64,
    outbox: std::sync::Arc<dyn ResultOutbox>,
}

impl AgentReportContext {
    pub fn new(
        config: &AgentRuntimeConfig,
        provider_count: usize,
        outbox: std::sync::Arc<dyn ResultOutbox>,
    ) -> Self {
        let broker_mode = if config.broker.broker_backend == "ws" {
            "relay"
        } else {
            "direct"
        };
        Self {
            started_at: Instant::now(),
            broker_mode: broker_mode.to_string(),
            broker_endpoint: config.broker.broker_endpoint.clone(),
            agent_version: config.version.clone(),
            config_hash: config_hash(config),
            provider_count,
            labels: config.labels.clone(),
            stale_after_seconds: config.stale_after.as_secs(),
            outbox,
        }
    }

    pub fn report(
        &self,
        status: &AgentStatus,
        heartbeat_seq: u64,
        clock_skew_ms: i64,
    ) -> AgentStatusReport {
        let (mut connection_state, reconnect_retry_seconds, reconnect_attempt, reconnect_budget) =
            match &status.connection {
                AgentConnection::Stopped => (AgentConnectionState::Stopped, None, None, None),
                AgentConnection::Registering => {
                    (AgentConnectionState::Registering, None, None, None)
                }
                AgentConnection::Connecting => (AgentConnectionState::Connecting, None, None, None),
                AgentConnection::Connected => (AgentConnectionState::Connected, None, None, None),
                AgentConnection::Reconnecting {
                    retry_secs,
                    attempt,
                    max_attempts,
                } => (
                    AgentConnectionState::Reconnecting,
                    Some(*retry_secs),
                    Some(*attempt),
                    *max_attempts,
                ),
                AgentConnection::Disconnected { attempts, .. } => (
                    AgentConnectionState::Disconnected,
                    None,
                    Some(*attempts),
                    Some(*attempts),
                ),
                AgentConnection::ReenrollmentRequired { .. } => {
                    (AgentConnectionState::ReenrollmentRequired, None, None, None)
                }
            };
        if self.outbox.is_full() {
            connection_state = AgentConnectionState::Draining;
        }
        AgentStatusReport {
            connection_state,
            reconnect_retry_seconds,
            reconnect_attempt,
            reconnect_max_attempts: reconnect_budget,
            broker_mode: self.broker_mode.clone(),
            broker_endpoint: self.broker_endpoint.clone(),
            in_flight: status.metrics.in_flight,
            succeeded: status.metrics.succeeded,
            failed: status.metrics.failed,
            timed_out: status.metrics.timed_out,
            canceled: status.metrics.canceled,
            last_error: status.last_error.clone(),
            last_error_at: status.last_error_at,
            outbox_depth: self.outbox.depth(),
            agent_version: self.agent_version.clone(),
            config_hash: self.config_hash.clone(),
            provider_count: self.provider_count,
            labels: self.labels.clone(),
            uptime_seconds: self.started_at.elapsed().as_secs(),
            heartbeat_seq,
            clock_skew_ms,
            stale_after_seconds: Some(self.stale_after_seconds),
        }
    }
}

fn config_hash(config: &AgentRuntimeConfig) -> String {
    let canonical = serde_json::json!({
        "service_url": config.service_url,
        "locator_mode": format!("{:?}", config.locator_mode),
        "gossip_bind": config.gossip_bind,
        "gossip_port": config.gossip_port,
        "instance_id": config.instance_id,
        "labels": config.labels,
        "exclusive": config.exclusive,
        "broker_backend": config.broker.broker_backend,
        "broker_endpoint": config.broker.broker_endpoint,
        "max_concurrent_actions": config.max_concurrent_actions,
        "shutdown_grace_seconds": config.shutdown_grace.as_secs(),
        "heartbeat_seconds": config.heartbeat_interval.as_secs(),
        "stale_after_seconds": config.stale_after.as_secs(),
        "outbox_file": config.outbox_file,
    });
    let digest = Sha256::digest(canonical.to_string().as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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
