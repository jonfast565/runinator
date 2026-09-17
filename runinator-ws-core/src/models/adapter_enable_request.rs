#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct AdapterEnableRequest {
    pub enabled: bool,
}

impl Validate for AdapterEnableRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
