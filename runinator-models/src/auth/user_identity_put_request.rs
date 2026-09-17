#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct UserIdentityPutRequest {
    pub provider: String,
    pub subject: String,
}

impl Validate for UserIdentityPutRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("provider", &self.provider, SHORT_TEXT_MAX)?;
        required_text("subject", &self.subject, SHORT_TEXT_MAX)
    }
}
