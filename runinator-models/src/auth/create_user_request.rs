#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub platform_role: Option<PlatformRole>,
}

impl Validate for CreateUserRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_human_platform_role(self.platform_role)?;
        identifier("username", &self.username)?;
        required_text("password", &self.password, LONG_TEXT_MAX)?;
        optional_email("email", self.email.as_deref())
    }
}
