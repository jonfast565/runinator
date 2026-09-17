#[allow(unused_imports)]
use super::*;

/// set the desired node count for a kind on a backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleNodesRequest {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub desired: u32,
    #[serde(default)]
    pub spec: NodeSpec,
}

impl Validate for ScaleNodesRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
