#[allow(unused_imports)]
use super::*;

/// resolve a pipeline run's pending inquiry (a member with the `Inquire` failure mode paused it).
/// mirrors [`ApprovalResolutionRequest`]'s shape; `decision` plays the approve/reject role.
#[derive(Debug, Deserialize)]
pub struct PipelineRunResolutionRequest {
    pub decision: PipelineRunInquiryDecision,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub override_reason: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

impl Validate for PipelineRunResolutionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("resolved_by", self.resolved_by.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("message", self.message.as_deref(), LONG_TEXT_MAX)?;
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
