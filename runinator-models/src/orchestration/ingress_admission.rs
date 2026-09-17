#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressAdmission {
    pub id: Option<Uuid>,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub scope: String,
    pub correlation_key: String,
    pub generation: i64,
    pub target: IngressTarget,
    pub status: IngressAdmissionStatus,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default)]
    pub policy: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
