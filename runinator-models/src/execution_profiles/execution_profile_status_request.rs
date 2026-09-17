#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileStatusRequest {
    pub health: ExecutionProfileHealth,
    #[serde(default)]
    pub error: Option<String>,
}

impl Validate for ExecutionProfileStatusRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(error) = &self.error {
            bounded_text("error", error, 512)?;
        }
        Ok(())
    }
}
