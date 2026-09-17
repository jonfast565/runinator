#[allow(unused_imports)]
use super::*;

/// one provisioned node instance within a group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionedNode {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub node_id: String,
    pub status: String,
}
