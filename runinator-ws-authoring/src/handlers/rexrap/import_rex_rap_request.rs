#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct ImportRexRapRequest {
    pub source: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
}

impl Validate for ImportRexRapRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_source("source", &self.source)?;
        if self.triggers.len() > 256 {
            return Err(ValidationError::new(
                "triggers",
                "must contain at most 256 triggers",
            ));
        }
        for (index, trigger) in self.triggers.iter().enumerate() {
            trigger.validate().map_err(|error| {
                ValidationError::new(format!("triggers[{index}].{}", error.path), error.message)
            })?;
        }
        Ok(())
    }
}
