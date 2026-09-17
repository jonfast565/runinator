#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

impl Validate for ChangePasswordRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("current_password", &self.current_password, LONG_TEXT_MAX)?;
        required_text("new_password", &self.new_password, LONG_TEXT_MAX)
    }
}
