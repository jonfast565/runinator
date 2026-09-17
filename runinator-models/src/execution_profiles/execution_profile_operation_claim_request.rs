#[allow(unused_imports)]
use super::*;

/// Binds an operation claim to the configuration the desktop agent approved locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileOperationClaimRequest {
    pub config_digest: String,
}

impl Validate for ExecutionProfileOperationClaimRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("config_digest", &self.config_digest, SHORT_TEXT_MAX)
    }
}
