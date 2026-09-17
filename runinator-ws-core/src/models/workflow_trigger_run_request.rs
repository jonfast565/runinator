#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct WorkflowTriggerRunRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub debug: bool,
}

impl Validate for WorkflowTriggerRunRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
