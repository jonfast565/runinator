#[allow(unused_imports)]
use super::*;

/// set the desired dedicated-node count for a kind in an org. `dedicated=false` reserved for future
/// shared-pool reservations; today only dedicated groups are provisioned per org.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleOrgNodesRequest {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub desired: u32,
}

impl Validate for ScaleOrgNodesRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
