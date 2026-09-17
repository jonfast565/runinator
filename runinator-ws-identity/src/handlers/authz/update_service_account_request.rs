#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct UpdateServiceAccountRequest {
    pub disabled: bool,
}

impl Validate for UpdateServiceAccountRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
