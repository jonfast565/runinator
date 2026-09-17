#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct RaceParameters {
    pub branches: Vec<WorkflowNodeRef>,
    pub winner: BranchPolicy,
}
