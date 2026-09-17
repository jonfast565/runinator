#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOrchestrationBinding {
    pub id: Uuid,
    pub admission_id: Uuid,
    pub org_id: Option<Uuid>,
    pub scope: String,
    pub correlation_key: String,
    pub generation: i64,
    pub pipeline_id: Uuid,
    pub pipeline_revision: i64,
    pub pipeline_digest: String,
    pub adapter_id: Option<Uuid>,
    pub adapter_revision: Option<i64>,
    pub policy: OrchestrationPolicy,
}
