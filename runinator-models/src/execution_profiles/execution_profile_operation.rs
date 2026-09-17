#[allow(unused_imports)]
use super::*;

/// One durable dry-run or refresh request, claimed and completed by one desktop agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileOperation {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub config_digest: String,
    pub kind: ExecutionProfileOperationKind,
    pub state: ExecutionProfileOperationState,
    pub requested_at: DateTime<Utc>,
    pub requested_by: Option<Uuid>,
    pub claimed_by: Option<Uuid>,
    pub started_at: Option<DateTime<Utc>>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}
