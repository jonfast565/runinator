#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct PercentageBucket {
    pub weight: i64,
    pub target: WorkflowNodeRef,
}
