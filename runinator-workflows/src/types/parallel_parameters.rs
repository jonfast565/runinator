#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelParameters {
    pub branches: Vec<WorkflowNodeRef>,
}
