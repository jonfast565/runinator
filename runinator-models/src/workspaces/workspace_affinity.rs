#[allow(unused_imports)]
use super::*;

/// Immutable routing token copied into a workflow action. The version and attempt prevent a stale
/// continuation from silently reusing a superseded local workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAffinity {
    pub workspace_id: Uuid,
    pub worker_instance_id: String,
    /// Opaque relative key resolved beneath the selected worker's configured workspace root.
    #[serde(default)]
    pub local_key: String,
    pub attempt: i64,
    pub version: i64,
}
