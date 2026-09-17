#[allow(unused_imports)]
use super::*;

/// A desktop agent's observation after it has inspected the local approval and collection state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileAgentStatusRequest {
    pub config_digest: String,
    pub approval: ExecutionProfileApprovalState,
    #[serde(default)]
    pub last_attempt_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_success_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl Validate for ExecutionProfileAgentStatusRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("config_digest", &self.config_digest, SHORT_TEXT_MAX)?;
        if let Some(error) = &self.last_error {
            bounded_text("last_error", error, 512)?;
        }
        Ok(())
    }
}
