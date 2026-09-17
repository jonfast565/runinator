#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateCurrentUserRequest {
    #[serde(default)]
    pub email: Option<Option<String>>,
}

impl Validate for UpdateCurrentUserRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_email(
            "email",
            self.email.as_ref().and_then(|value| value.as_deref()),
        )
    }
}
