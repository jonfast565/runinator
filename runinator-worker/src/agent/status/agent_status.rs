#[allow(unused_imports)]
use super::*;

/// a snapshot of the agent lifecycle, republished on every transition.
#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    /// true once the replica is registered and the action loop has been handed its first attempt.
    pub running: bool,
    pub replica_id: Option<Uuid>,
    pub connection: AgentConnection,
    /// how this agent reaches the broker, e.g. `relay via wss://host/ws/broker`.
    pub broker_connection: Option<String>,
    pub metrics: AgentMetrics,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
}
