#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateTeamRequest {
    pub name: String,
}

impl Validate for UpdateTeamRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)
    }
}
