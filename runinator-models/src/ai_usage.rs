//! durable, provider-neutral AI token and cost accounting.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

mod ai_token_usage;
pub use ai_token_usage::AiTokenUsage;

mod ai_usage;
pub use ai_usage::AiUsage;

mod ai_rate_entry;
pub use ai_rate_entry::AiRateEntry;

mod ai_usage_record;
pub use ai_usage_record::AiUsageRecord;

mod ai_usage_totals;
pub use ai_usage_totals::AiUsageTotals;

mod ai_usage_breakdown;
pub use ai_usage_breakdown::AiUsageBreakdown;

mod ai_usage_report;
pub use ai_usage_report::AiUsageReport;
