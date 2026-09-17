#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub target: WorkflowNodeRef,
    pub condition: WorkflowCondition,
}
