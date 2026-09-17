#[allow(unused_imports)]
use super::*;

/// one option of a small closed enum (gate kind, edge match kind, branch policy, setting kind,
/// interrupt source, resume mode, concurrency policy).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumOptionMetadata {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl EnumOptionMetadata {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.to_string(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }
}
