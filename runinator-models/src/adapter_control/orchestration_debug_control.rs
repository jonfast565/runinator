#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestrationDebugControl {
    pub paused: bool,
    pub steps: i64,
}

impl crate::validation::Validate for OrchestrationDebugControl {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        if !(0..=1).contains(&self.steps) || (!self.paused && self.steps != 0) {
            return Err(crate::validation::ValidationError::new(
                "steps",
                "must be zero or one; stepping requires pause",
            ));
        }
        Ok(())
    }
}
