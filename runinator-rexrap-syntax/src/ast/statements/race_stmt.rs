#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct RaceStmt {
    pub branches: Vec<Block>,
    pub winner: BranchPolicy,
}
