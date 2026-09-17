#[allow(unused_imports)]
use super::*;

/// persisted directive intent and its eventual agent reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDirectiveRecord {
    pub directive_id: Uuid,
    pub replica_id: Uuid,
    pub kind: AgentDirectiveKind,
    pub state: AgentDirectiveState,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub attempts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by_runtime_id: Option<String>,
}
