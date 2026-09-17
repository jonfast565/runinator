#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct TryParameters {
    pub body: WorkflowNodeRef,
    pub catch: Option<WorkflowNodeRef>,
    pub finally: Option<WorkflowNodeRef>,
}
