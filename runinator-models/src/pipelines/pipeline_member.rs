#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineMember {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Value>,
    /// stable pipeline-local identity and expression key: the authored canonical workflow path.
    pub key: String,
    pub workflow_id: Uuid,
    #[serde(default)]
    pub failure_mode: PipelineMemberFailureMode,
}
