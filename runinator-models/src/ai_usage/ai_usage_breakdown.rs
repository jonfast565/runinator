#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiUsageBreakdown {
    pub key: String,
    pub totals: AiUsageTotals,
}
