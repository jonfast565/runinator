#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct IdempotencyRequest {
    pub consumer_run_id: Uuid,
    pub scope: String,
    pub key: String,
    #[serde(default)]
    pub result: Value,
}

impl Validate for IdempotencyRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scope", &self.scope)?;
        required_text("key", &self.key, SHORT_TEXT_MAX)
    }
}
