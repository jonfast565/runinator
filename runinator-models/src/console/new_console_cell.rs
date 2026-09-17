#[allow(unused_imports)]
use super::*;

/// what a caller sends to create or replace a cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewConsoleCell {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// append when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
}

impl Validate for NewConsoleCell {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("source", &self.source, LONG_TEXT_MAX)?;
        optional_text("label", self.label.as_deref(), SHORT_TEXT_MAX)?;
        if self.position.is_some_and(|position| position < 0) {
            return Err(ValidationError::new("position", "must not be negative"));
        }
        Ok(())
    }
}
