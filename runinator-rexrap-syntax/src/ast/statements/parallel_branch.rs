#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelBranch {
    pub label: Option<String>,
    pub body: Block,
}
