#[allow(unused_imports)]
use super::*;

/// the stored replay payload for a completed action: enough to settle a redelivered node run
/// exactly as the original execution settled it, without re-invoking the provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdempotentActionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_commit: Option<crate::workspaces::WorkspaceCommit>,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
