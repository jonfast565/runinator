#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationCommand {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub epoch: i64,
    pub command_type: String,
    pub operation_key: String,
    #[serde(default)]
    pub payload: Value,
    pub status: OrchestrationCommandStatus,
    pub attempts: i64,
    #[serde(default)]
    pub claimed_by: Option<String>,
    #[serde(default)]
    pub claimed_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub result: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
