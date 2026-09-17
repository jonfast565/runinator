#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationBinding {
    pub id: Uuid,
    pub admission_id: Uuid,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub scope: String,
    pub correlation_key: String,
    pub generation: i64,
    pub pipeline_id: Uuid,
    pub pipeline_revision: i64,
    pub pipeline_digest: String,
    #[serde(default)]
    pub adapter_id: Option<Uuid>,
    #[serde(default)]
    pub adapter_revision: Option<i64>,
    pub policy: OrchestrationPolicy,
    pub status: OrchestrationStatus,
    #[serde(default)]
    pub current_phase: Option<String>,
    pub current_attempt: i64,
    pub current_epoch: i64,
    #[serde(default)]
    pub restart_member: Option<String>,
    #[serde(default)]
    pub resume_existing_epoch: bool,
    #[serde(default)]
    pub subject_revision: Option<String>,
    #[serde(default)]
    pub resources: Value,
    #[serde(default)]
    pub budgets: BTreeMap<String, u32>,
    pub last_reduced_sequence: i64,
    pub version: i64,
    #[serde(default)]
    pub reducer_lease_owner: Option<String>,
    #[serde(default)]
    pub reducer_leased_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
}
