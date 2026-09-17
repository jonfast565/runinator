#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetPolicy {
    pub attempts: u32,
    pub exhausted: BudgetExhaustion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<String>,
}
