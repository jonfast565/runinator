#[allow(unused_imports)]
use super::*;

/// one provisionable node group and its current sizing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionedGroup {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    /// backend-local group name (deployment name or supervised process prefix).
    pub name: String,
    /// requested node count.
    pub desired: u32,
    /// nodes currently reporting available/running.
    pub available: u32,
    /// false when the backend can observe but not change this group.
    pub manageable: bool,
    /// the smallest desired count the UI should allow (a floor of one for control-plane kinds).
    /// backend-provided so a new protected kind needs no UI change.
    #[serde(default)]
    pub min_desired: u32,
}
