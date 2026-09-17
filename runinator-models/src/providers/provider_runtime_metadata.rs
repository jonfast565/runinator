#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRuntimeMetadata {
    #[serde(default)]
    pub credential_scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    /// How this provider safely consumes an effect-private execution profile.
    #[serde(default)]
    pub execution_profile: ExecutionProfileSupport,
}
