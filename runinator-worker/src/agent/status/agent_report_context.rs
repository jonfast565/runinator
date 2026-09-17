#[allow(unused_imports)]
use super::*;

pub struct AgentReportContext {
    pub(super) started_at: Instant,
    pub(super) broker_mode: String,
    pub(super) broker_endpoint: String,
    pub(super) agent_version: Option<String>,
    pub(super) worker_settings: RwLock<ActiveWorkerSettings>,
    pub(super) provider_count: usize,
    pub(super) labels: std::collections::BTreeMap<String, String>,
    pub(super) stale_after_seconds: u64,
    pub(super) outbox: std::sync::Arc<dyn ResultOutbox>,
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
            worker_settings: RwLock::new(active_worker_settings(
                config,
                initial_settings_source(config),
            )),
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
        let worker_settings = self
            .worker_settings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            config_hash: worker_settings.config_hash.clone(),
            max_concurrent_actions: Some(worker_settings.max_concurrent_actions),
            shutdown_grace_seconds: Some(worker_settings.shutdown_grace_seconds),
            worker_settings_source: Some(worker_settings.source.clone()),
            provider_count: self.provider_count,
            labels: self.labels.clone(),
            uptime_seconds: self.started_at.elapsed().as_secs(),
            heartbeat_seq,
            clock_skew_ms,
            stale_after_seconds: Some(self.stale_after_seconds),
        }
    }

    pub fn update_worker_settings(&self, config: &AgentRuntimeConfig, source: &str) {
        *self
            .worker_settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            active_worker_settings(config, source);
    }
}
