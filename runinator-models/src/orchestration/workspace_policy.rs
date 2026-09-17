#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspacePolicy {
    pub scope: String,
    #[serde(default)]
    pub requirements: Value,
    #[serde(default = "default_workspace_lease_seconds")]
    pub lease_seconds: u64,
    #[serde(default)]
    pub reuse: bool,
    #[serde(default)]
    pub recovery: WorkspaceRecovery,
}
