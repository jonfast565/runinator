#[allow(unused_imports)]
use super::*;

/// request body for duplicating a workflow into a new version sharing the same name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowDuplicateRequest {
    #[serde(default)]
    pub bump: SemVerBump,
}
