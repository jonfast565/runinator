#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct WorkflowRunReplayRequest {
    #[serde(default)]
    pub plan_fingerprint: Option<String>,
    #[serde(default)]
    pub acknowledge_review: bool,
    #[serde(default)]
    pub from_step_id: Option<String>,
    #[serde(default)]
    pub override_reason: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

impl Validate for WorkflowRunReplayRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("from_step_id", self.from_step_id.as_deref(), SHORT_TEXT_MAX)?;
        optional_text(
            "plan_fingerprint",
            self.plan_fingerprint.as_deref(),
            SHORT_TEXT_MAX,
        )?;
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
