#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

impl Validate for LoginRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("username", &self.username, SHORT_TEXT_MAX)?;
        required_text("password", &self.password, LONG_TEXT_MAX)
    }
}
