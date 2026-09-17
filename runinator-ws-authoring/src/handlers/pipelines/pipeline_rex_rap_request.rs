#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct PipelineRexRapRequest {
    pub source: String,
}

impl runinator_models::validation::Validate for PipelineRexRapRequest {
    fn validate(&self) -> Result<(), runinator_models::validation::ValidationError> {
        runinator_models::validation::required_text("source", &self.source, 2 * 1024 * 1024)?;
        runinator_models::validation::bounded_text("source", &self.source, 2 * 1024 * 1024)
    }
}
