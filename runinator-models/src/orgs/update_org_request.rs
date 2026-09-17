#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateOrgRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

impl Validate for UpdateOrgRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("name", self.name.as_deref(), SHORT_TEXT_MAX)
    }
}
