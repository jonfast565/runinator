//! the agent's externally visible state: where the lifecycle is in the connect/retry cycle, and the
//! running action counters. both the headless binary and a gui host read the same types, so a
//! degraded agent looks the same in a log line as it does in a status header.

use std::{sync::RwLock, time::Instant};

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

/// immutable facts combined with each live status snapshot to build the wire report.

fn initial_settings_source(config: &AgentRuntimeConfig) -> &'static str {
    if config.use_server_worker_settings {
        "process"
    } else {
        "desktop"
    }
}

fn active_worker_settings(config: &AgentRuntimeConfig, source: &str) -> ActiveWorkerSettings {
    ActiveWorkerSettings {
        config_hash: config_hash(config),
        max_concurrent_actions: config.max_concurrent_actions as u64,
        shutdown_grace_seconds: config.shutdown_grace.as_secs(),
        source: source.to_string(),
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
        "worker_settings_refresh_seconds": config.worker_settings_refresh_interval.as_secs(),
        "heartbeat_seconds": config.heartbeat_interval.as_secs(),
        "stale_after_seconds": config.stale_after.as_secs(),
        "outbox_file": config.outbox_file,
    });
    let digest = Sha256::digest(canonical.to_string().as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// first UUID segment; enough to correlate a console line with the run in the command center.
pub fn short_id(id: &Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;

mod agent_status;
pub use agent_status::AgentStatus;

mod agent_report_context;
pub use agent_report_context::AgentReportContext;

mod active_worker_settings;
use active_worker_settings::ActiveWorkerSettings;

mod completed_action;
pub use completed_action::CompletedAction;

mod agent_metrics;
pub use agent_metrics::AgentMetrics;
