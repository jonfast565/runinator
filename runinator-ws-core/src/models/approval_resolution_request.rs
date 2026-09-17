#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct ApprovalResolutionRequest {
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub output_json: Option<Value>,
}

impl Validate for ApprovalResolutionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("resolved_by", self.resolved_by.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("message", self.message.as_deref(), LONG_TEXT_MAX)
    }
}
