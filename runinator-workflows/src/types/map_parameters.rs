#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MapParameters {
    pub items: WorkflowExpression,
    pub target: WorkflowNodeRef,
    pub concurrency: Option<i64>,
}
