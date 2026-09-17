#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowJoinFrame {
    pub join_key: String,
    pub expected: u64,
    #[serde(default)]
    pub mode: WorkflowBranchPolicy,
    #[serde(default)]
    pub arrivals: Vec<WorkflowIndexedValue>,
}
