#[allow(unused_imports)]
use super::*;

/// backward-compatible agent health payload carried inside replica registration/heartbeat
/// attributes. servers that predate it preserve the object without interpreting it, while newer
/// clients can render agent-specific health without widening the replica persistence contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentStatusReport {
    pub connection_state: AgentConnectionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_retry_seconds: Option<u64>,
    /// which consecutive reconnect attempt is pending (1-based), while reconnecting or disconnected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_attempt: Option<u32>,
    /// the agent's reconnect budget; `None` when it retries indefinitely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_max_attempts: Option<u32>,
    pub broker_mode: String,
    pub broker_endpoint: String,
    pub in_flight: u32,
    pub succeeded: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub canceled: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub outbox_depth: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    pub config_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_actions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutdown_grace_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_settings_source: Option<String>,
    pub provider_count: usize,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub uptime_seconds: u64,
    pub heartbeat_seq: u64,
    /// estimated server minus agent wall-clock offset.
    #[serde(default)]
    pub clock_skew_ms: i64,
    /// how long this agent expects to remain live without a heartbeat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_after_seconds: Option<u64>,
}
