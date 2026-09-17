#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct JoinParameters {
    pub wait_for: Vec<WorkflowNodeRef>,
    pub mode: BranchPolicy,
}
