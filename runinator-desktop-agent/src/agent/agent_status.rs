#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    pub running: bool,
    pub replica_id: Option<Uuid>,
    pub root: Option<String>,
    /// e.g. "relay via wss://.../ws/broker" or "direct TCP @ host:port".
    pub broker_connection: Option<String>,
}
