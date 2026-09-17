#[allow(unused_imports)]
use super::*;

/// request body for reserving an action node's idempotency key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyClaimRequest {
    pub consumer_run_id: Uuid,
    pub scope: String,
    pub key: String,
    pub owner_node_run_id: Uuid,
    /// the claimant's own execution deadline in seconds; a reservation older than this is treated as
    /// abandoned and taken over. defaults to the action default timeout for older callers.
    #[serde(default = "default_idempotency_lease_seconds")]
    pub lease_seconds: i64,
}

impl Validate for IdempotencyClaimRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scope", &self.scope)?;
        required_text("key", &self.key, SHORT_TEXT_MAX)?;
        if !(1..=86_400).contains(&self.lease_seconds) {
            return Err(ValidationError::new(
                "lease_seconds",
                "must be between 1 and 86400",
            ));
        }
        Ok(())
    }
}
