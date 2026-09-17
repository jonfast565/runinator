#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusRequest {
    pub status: WorkflowStatus,
    #[serde(default)]
    pub active_node_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

impl Validate for WorkflowRunStatusRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text(
            "active_node_id",
            self.active_node_id.as_deref(),
            SHORT_TEXT_MAX,
        )?;
        optional_text("message", self.message.as_deref(), LONG_TEXT_MAX)
    }
}
