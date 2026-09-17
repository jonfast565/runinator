#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct OrchestrationRequeueRequest {
    pub reason: String,
    pub idempotency_key: String,
}

impl Validate for OrchestrationRequeueRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("reason", &self.reason, LONG_TEXT_MAX)?;
        required_text("idempotency_key", &self.idempotency_key, SHORT_TEXT_MAX)
    }
}
