#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiUsageTotals {
    pub requests: u64,
    pub unpriced_requests: u64,
    pub tokens: AiTokenUsage,
    /// Sum of priced records. `None` means every matching record was unpriced.
    #[serde(default)]
    pub cost_microusd: Option<u64>,
}

impl AiUsageTotals {
    pub fn add(&mut self, record: &AiUsageRecord) {
        self.requests = self.requests.saturating_add(1);
        self.tokens.input_tokens = self
            .tokens
            .input_tokens
            .saturating_add(record.tokens.input_tokens);
        self.tokens.cached_input_tokens = self
            .tokens
            .cached_input_tokens
            .saturating_add(record.tokens.cached_input_tokens);
        self.tokens.cache_creation_input_tokens = self
            .tokens
            .cache_creation_input_tokens
            .saturating_add(record.tokens.cache_creation_input_tokens);
        self.tokens.output_tokens = self
            .tokens
            .output_tokens
            .saturating_add(record.tokens.output_tokens);
        self.tokens.reasoning_tokens = self
            .tokens
            .reasoning_tokens
            .saturating_add(record.tokens.reasoning_tokens);
        match record.cost_microusd {
            Some(cost) => {
                self.cost_microusd =
                    Some(self.cost_microusd.unwrap_or_default().saturating_add(cost));
            }
            None => self.unpriced_requests = self.unpriced_requests.saturating_add(1),
        }
    }
}
