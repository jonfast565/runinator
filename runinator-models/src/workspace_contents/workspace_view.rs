#[allow(unused_imports)]
use super::*;

/// Caller-specific management projection; authorization is never persisted on the identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceView {
    #[serde(flatten)]
    pub workspace: DurableWorkspace,
    pub permission: crate::auth::Permission,
}
