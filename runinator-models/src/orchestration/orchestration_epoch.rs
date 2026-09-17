#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationEpoch {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub epoch: i64,
    #[serde(default)]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default)]
    pub start_member: Option<String>,
    #[serde(default)]
    pub parameters: Value,
    pub status: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
}
