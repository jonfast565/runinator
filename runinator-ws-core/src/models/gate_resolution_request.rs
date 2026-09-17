#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize, ToSchema)]
pub struct GateResolutionRequest {
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Validate for GateResolutionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("resolved_by", self.resolved_by.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("reason", self.reason.as_deref(), LONG_TEXT_MAX)
    }
}
