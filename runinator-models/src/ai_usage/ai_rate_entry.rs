#[allow(unused_imports)]
use super::*;

/// A platform-wide model price in micro-USD per million tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiRateEntry {
    pub provider: String,
    /// Exact model identifier, or `*` as a provider fallback.
    pub model: String,
    pub input_microusd_per_million_tokens: u64,
    pub cached_input_microusd_per_million_tokens: u64,
    pub cache_creation_input_microusd_per_million_tokens: u64,
    pub output_microusd_per_million_tokens: u64,
    pub reasoning_microusd_per_million_tokens: u64,
}

impl AiRateEntry {
    pub fn price(&self, tokens: &AiTokenUsage) -> u64 {
        let total = u128::from(tokens.input_tokens)
            * u128::from(self.input_microusd_per_million_tokens)
            + u128::from(tokens.cached_input_tokens)
                * u128::from(self.cached_input_microusd_per_million_tokens)
            + u128::from(tokens.cache_creation_input_tokens)
                * u128::from(self.cache_creation_input_microusd_per_million_tokens)
            + u128::from(tokens.output_tokens)
                * u128::from(self.output_microusd_per_million_tokens)
            + u128::from(tokens.reasoning_tokens)
                * u128::from(self.reasoning_microusd_per_million_tokens);
        // round to the nearest micro-dollar rather than systematically undercharging.
        u64::try_from((total + 500_000) / 1_000_000).unwrap_or(u64::MAX)
    }
}
