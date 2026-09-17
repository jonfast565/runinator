#[allow(unused_imports)]
use super::*;

/// a literal light switch: `value` truthiness routes to `on`, otherwise `off`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToggleParameters {
    pub value: WorkflowExpression,
    pub on: WorkflowNodeRef,
    pub off: WorkflowNodeRef,
}
