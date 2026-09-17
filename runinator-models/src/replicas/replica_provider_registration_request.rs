#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaProviderRegistrationRequest {
    pub runtime_id: String,
    pub provider: ProviderMetadata,
}

impl Validate for ReplicaProviderRegistrationRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("runtime_id", &self.runtime_id)?;
        required_text("provider.name", &self.provider.name, SHORT_TEXT_MAX)
    }
}
