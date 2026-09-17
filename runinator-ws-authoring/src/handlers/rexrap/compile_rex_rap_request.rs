#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct CompileRexRapRequest {
    pub source: String,
    #[serde(default)]
    pub enabled: bool,
}

impl Validate for CompileRexRapRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_source("source", &self.source)
    }
}
