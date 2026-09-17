#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationEvidence {
    pub id: Uuid,
    pub binding_id: Uuid,
    #[serde(default)]
    pub epoch: Option<i64>,
    pub kind: String,
    #[serde(default)]
    pub subject_revision: Option<String>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub source_event_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}
