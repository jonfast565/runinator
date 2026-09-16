//! durable, provider-neutral AI token and cost accounting.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Token categories reported by an AI execution backend.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
}

impl AiTokenUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.cached_input_tokens)
            .saturating_add(self.cache_creation_input_tokens)
            .saturating_add(self.output_tokens)
            .saturating_add(self.reasoning_tokens)
    }

    pub fn is_empty(&self) -> bool {
        self.total_tokens() == 0
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiCostSource {
    ProviderReported,
    RateCard,
}

impl AiCostSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderReported => "provider_reported",
            Self::RateCard => "rate_card",
        }
    }
}

impl TryFrom<&str> for AiCostSource {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "provider_reported" => Ok(Self::ProviderReported),
            "rate_card" => Ok(Self::RateCard),
            other => Err(format!("unknown AI cost source '{other}'")),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiUsageRecord {
    pub event_id: Uuid,
    pub effect_id: Uuid,
    pub workflow_run_id: Uuid,
    pub workflow_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub attempt: u32,
    pub provider: String,
    pub model: String,
    pub tokens: AiTokenUsage,
    #[serde(default)]
    pub cost_microusd: Option<u64>,
    #[serde(default)]
    pub cost_source: Option<AiCostSource>,
    pub recorded_at: DateTime<Utc>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiUsageBreakdown {
    pub key: String,
    pub totals: AiUsageTotals,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiUsageReport {
    pub totals: AiUsageTotals,
    #[serde(default)]
    pub records: Vec<AiUsageRecord>,
    #[serde(default)]
    pub by_run: Vec<AiUsageBreakdown>,
    #[serde(default)]
    pub by_node: Vec<AiUsageBreakdown>,
    #[serde(default)]
    pub by_provider: Vec<AiUsageBreakdown>,
    #[serde(default)]
    pub by_model: Vec<AiUsageBreakdown>,
}
