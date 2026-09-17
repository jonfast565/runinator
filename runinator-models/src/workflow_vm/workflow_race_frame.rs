#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRaceFrame {
    pub race_key: String,
    pub expected: u64,
    #[serde(default = "WorkflowBranchPolicy::first_success")]
    pub winner_policy: WorkflowBranchPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_value: Option<Value>,
}
