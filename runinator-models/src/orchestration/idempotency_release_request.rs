#[allow(unused_imports)]
use super::*;

/// request body for releasing an unfinished reservation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyReleaseRequest {
    pub consumer_run_id: Uuid,
    pub scope: String,
    pub key: String,
    pub owner_node_run_id: Uuid,
}

impl Validate for IdempotencyReleaseRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scope", &self.scope)?;
        required_text("key", &self.key, SHORT_TEXT_MAX)
    }
}
