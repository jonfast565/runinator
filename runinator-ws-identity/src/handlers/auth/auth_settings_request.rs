#[allow(unused_imports)]
use super::*;

#[derive(serde::Deserialize)]
pub struct AuthSettingsRequest {
    pub max_refreshes: i64,
}

impl Validate for AuthSettingsRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if !(1..=100_000).contains(&self.max_refreshes) {
            return Err(ValidationError::new(
                "max_refreshes",
                "must be between 1 and 100000",
            ));
        }
        Ok(())
    }
}
