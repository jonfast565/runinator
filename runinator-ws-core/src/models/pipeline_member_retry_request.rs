#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default, Deserialize)]
pub struct PipelineMemberRetryRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub override_reason: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

impl Validate for PipelineMemberRetryRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text(
            "override_reason",
            self.override_reason.as_deref(),
            LONG_TEXT_MAX,
        )?;
        optional_text(
            "idempotency_key",
            self.idempotency_key.as_deref(),
            SHORT_TEXT_MAX,
        )
    }
}
