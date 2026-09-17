#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct ExternalOperationResolutionRequest {
    pub resolution: String,
    pub reason: String,
    #[serde(default)]
    pub receipt: Value,
}

impl Validate for ExternalOperationResolutionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if !matches!(self.resolution.as_str(), "succeeded" | "failed" | "retry") {
            return Err(ValidationError::new(
                "resolution",
                "must be one of succeeded, failed, or retry",
            ));
        }
        required_text("reason", &self.reason, LONG_TEXT_MAX)
    }
}
