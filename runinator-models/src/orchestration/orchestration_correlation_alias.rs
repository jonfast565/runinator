#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationCorrelationAlias {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub generation: i64,
    pub org_id: Option<Uuid>,
    pub source: String,
    pub scope: String,
    pub correlation_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
