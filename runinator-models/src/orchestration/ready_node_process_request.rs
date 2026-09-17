#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyNodeProcessRequest {
    pub scheduler_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_ready_at: Option<DateTime<Utc>>,
}

impl Validate for ReadyNodeProcessRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scheduler_id", &self.scheduler_id)?;
        if let Some(node_id) = self.node_id.as_deref() {
            identifier("node_id", node_id)?;
        }
        Ok(())
    }
}
