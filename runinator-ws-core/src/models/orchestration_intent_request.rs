#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct OrchestrationIntentRequest {
    pub intent: String,
    #[serde(default)]
    pub payload: Value,
    pub reason: String,
    pub idempotency_key: String,
}

impl Validate for OrchestrationIntentRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("intent", &self.intent)?;
        required_text("reason", &self.reason, LONG_TEXT_MAX)?;
        required_text("idempotency_key", &self.idempotency_key, SHORT_TEXT_MAX)
    }
}
