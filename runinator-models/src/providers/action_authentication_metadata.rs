#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionAuthenticationMetadata {
    #[serde(default = "default_true")]
    pub required: bool,
    pub alternatives: Vec<ActionAuthenticationAlternative>,
    /// Whether more than one satisfied alternative may be supplied. Later credential injection
    /// takes precedence when two alternatives target the same subprocess setting.
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_multiple: bool,
}

impl ActionAuthenticationMetadata {
    pub fn required(alternatives: Vec<ActionAuthenticationAlternative>) -> Self {
        Self {
            required: true,
            alternatives,
            allow_multiple: false,
        }
    }

    pub fn optional(alternatives: Vec<ActionAuthenticationAlternative>) -> Self {
        Self {
            required: false,
            alternatives,
            allow_multiple: false,
        }
    }

    pub fn allow_multiple(mut self) -> Self {
        self.allow_multiple = true;
        self
    }
}
