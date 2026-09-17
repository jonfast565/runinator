#[allow(unused_imports)]
use super::*;

/// Completes a claimed desktop operation without altering the profile's publication availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileOperationCompleteRequest {
    pub state: ExecutionProfileOperationState,
    #[serde(default)]
    pub error: Option<String>,
}

impl Validate for ExecutionProfileOperationCompleteRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.state.is_active() {
            return Err(ValidationError::new(
                "state",
                "must be a terminal operation state",
            ));
        }
        if let Some(error) = &self.error {
            bounded_text("error", error, 512)?;
        }
        if self.state == ExecutionProfileOperationState::Failed && self.error.is_none() {
            return Err(ValidationError::new(
                "error",
                "is required when state is failed",
            ));
        }
        Ok(())
    }
}
