#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaOfflineRequest {
    pub runtime_id: String,
}

impl Validate for ReplicaOfflineRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("runtime_id", &self.runtime_id)
    }
}
