#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowBranch {
    pub when: WorkflowCondition,
    pub target: WorkflowNodeRef,
    /// selection priority for predicate edges; lower numbers are evaluated first. unset branches
    /// keep their declaration order (sorted after any numbered branches).
    #[serde(default)]
    pub priority: Option<i64>,
}
