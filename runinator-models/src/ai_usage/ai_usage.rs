#[allow(unused_imports)]
use super::*;

/// Usage attached by a provider to one terminal effect result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiUsage {
    pub provider: String,
    pub model: String,
    pub tokens: AiTokenUsage,
    /// Exact upstream charge, when the provider reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_cost_microusd: Option<u64>,
}
