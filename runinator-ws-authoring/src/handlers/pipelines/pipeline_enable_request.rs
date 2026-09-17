#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct PipelineEnableRequest {
    pub enabled: bool,
}

impl runinator_models::validation::Validate for PipelineEnableRequest {
    fn validate(&self) -> Result<(), runinator_models::validation::ValidationError> {
        Ok(())
    }
}
