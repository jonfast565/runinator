#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMemberAttempt {
    pub id: Uuid,
    pub pipeline_run_id: Uuid,
    pub member_key: String,
    pub workflow_id: Uuid,
    pub attempt: i64,
    pub workflow_run_id: Option<Uuid>,
    pub status: PipelineMemberAttemptStatus,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub result: Value,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}
