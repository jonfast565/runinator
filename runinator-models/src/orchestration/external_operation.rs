#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalOperation {
    pub id: Uuid,
    pub binding_id: Uuid,
    /// Immutable execution coordinates used to reject stale receipts and operator retries.
    pub epoch: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<Uuid>,
    pub operation_key: String,
    pub provider: String,
    pub action: String,
    pub semantics: DeliverySemantics,
    pub attempt: i64,
    pub status: ExternalOperationStatus,
    pub ambiguous: bool,
    #[serde(default)]
    pub provenance: Value,
    #[serde(default)]
    pub receipt: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
