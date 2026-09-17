#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default, Deserialize)]
pub struct PipelineRunRequest {
    #[serde(default)]
    pub parameters: Value,
    /// Run an immutable historical pipeline definition instead of the current head.
    #[serde(default)]
    pub revision: Option<i64>,
    /// Start with this member as the sole frontier instead of the graph's entry members.
    #[serde(default)]
    pub start_member: Option<String>,
}

impl Validate for PipelineRunRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.revision.is_some_and(|revision| revision <= 0) {
            return Err(ValidationError::new(
                "revision",
                "must be greater than zero",
            ));
        }
        if let Some(member) = self.start_member.as_deref() {
            identifier("start_member", member)?;
        }
        Ok(())
    }
}
