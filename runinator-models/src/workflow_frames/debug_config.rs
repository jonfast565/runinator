#[allow(unused_imports)]
use super::*;

/// user-owned debug configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DebugMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breakpoints: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pause_on_failure: bool,
}
