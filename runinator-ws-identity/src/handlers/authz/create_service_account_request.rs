#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct CreateServiceAccountRequest {
    pub name: String,
}

impl Validate for CreateServiceAccountRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)
    }
}
