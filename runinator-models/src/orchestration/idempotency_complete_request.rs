#[allow(unused_imports)]
use super::*;

/// request body for recording a completed execution against a reserved key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyCompleteRequest {
    pub consumer_run_id: Uuid,
    pub scope: String,
    pub key: String,
    pub owner_node_run_id: Uuid,
    #[serde(default)]
    pub result: Value,
}

impl Validate for IdempotencyCompleteRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scope", &self.scope)?;
        required_text("key", &self.key, SHORT_TEXT_MAX)
    }
}
