#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchParameters {
    pub value: WorkflowExpression,
    pub cases: Vec<SwitchCase>,
    pub default: Option<WorkflowNodeRef>,
}
