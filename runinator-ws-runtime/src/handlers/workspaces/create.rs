#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct Create {
    pub(super) key: String,
}

impl runinator_models::validation::Validate for Create {
    fn validate(&self) -> Result<(), runinator_models::validation::ValidationError> {
        runinator_models::validation::required_text("key", &self.key, 200)
    }
}
