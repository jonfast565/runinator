#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct CreateAgentDirectiveRequest {
    pub kind: AgentDirectiveKind,
    /// relative deadline for delivery and execution; defaults to five minutes.
    #[serde(default)]
    pub expires_in_seconds: Option<u64>,
}

impl Validate for CreateAgentDirectiveRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(seconds) = self.expires_in_seconds
            && !(1..=86_400).contains(&seconds)
        {
            return Err(ValidationError::new(
                "expires_in_seconds",
                "must be between 1 and 86400",
            ));
        }
        Ok(())
    }
}
