#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

impl Validate for RefreshRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("refresh_token", &self.refresh_token, LONG_TEXT_MAX)
    }
}
