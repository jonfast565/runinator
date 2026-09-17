#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ManagedRunOverrideRequest {
    /// Required when a platform administrator deliberately bypasses orchestration ownership.
    #[serde(default)]
    pub reason: Option<String>,
    /// Client-generated key used to prevent a retried override request from applying twice.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

impl Validate for ManagedRunOverrideRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("reason", self.reason.as_deref(), LONG_TEXT_MAX)?;
        optional_text(
            "idempotency_key",
            self.idempotency_key.as_deref(),
            SHORT_TEXT_MAX,
        )
    }
}
