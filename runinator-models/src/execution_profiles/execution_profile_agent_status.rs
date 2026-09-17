#[allow(unused_imports)]
use super::*;

/// The latest locally reported collection state from one desktop agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileAgentStatus {
    pub profile_id: Uuid,
    pub agent_id: Uuid,
    pub config_digest: String,
    pub approval: ExecutionProfileApprovalState,
    pub last_seen_at: DateTime<Utc>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}
