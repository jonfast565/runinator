#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelStmt {
    pub branches: Vec<ParallelBranch>,
    pub join: BranchPolicy,
    /// `None` retains the historical implicit "all branches" join.
    pub selected_branches: Option<Vec<String>>,
}
