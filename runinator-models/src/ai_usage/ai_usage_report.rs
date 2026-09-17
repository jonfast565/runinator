#[allow(unused_imports)]
use super::*;

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
