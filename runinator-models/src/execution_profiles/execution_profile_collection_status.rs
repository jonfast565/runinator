#[allow(unused_imports)]
use super::*;

/// The collection status presented to profile authors alongside immutable publication metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileCollectionStatus {
    pub profile_id: Uuid,
    pub config_digest: String,
    pub publication_health: ExecutionProfileHealth,
    pub current_revision: Option<i64>,
    pub published_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub latest_operation: Option<ExecutionProfileOperation>,
    pub agents: Vec<ExecutionProfileAgentStatus>,
}
