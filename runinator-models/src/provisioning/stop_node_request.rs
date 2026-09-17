#[allow(unused_imports)]
use super::*;

/// stop/remove a single provisioned node instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopNodeRequest {
    pub backend: ProvisionBackend,
    pub node_id: String,
}

impl Validate for StopNodeRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("node_id", &self.node_id)
    }
}
