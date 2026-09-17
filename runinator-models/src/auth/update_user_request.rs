#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub email: Option<Option<String>>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default, deserialize_with = "deserialize_platform_role_patch")]
    pub platform_role: Option<Option<PlatformRole>>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

impl Validate for UpdateUserRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_human_platform_role(self.platform_role.flatten())?;
        optional_email(
            "email",
            self.email.as_ref().and_then(|value| value.as_deref()),
        )?;
        optional_text("password", self.password.as_deref(), LONG_TEXT_MAX)
    }
}
