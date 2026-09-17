#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize, ToSchema)]
pub struct WorkflowRunRenameRequest {
    #[serde(default)]
    pub name: Option<String>,
}

impl Validate for WorkflowRunRenameRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("name", self.name.as_deref(), SHORT_TEXT_MAX)
    }
}
