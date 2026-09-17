#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct DecompileRexRapRequest {
    pub workflow: WorkflowDefinition,
}

impl Validate for DecompileRexRapRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        self.workflow.validate()
    }
}
