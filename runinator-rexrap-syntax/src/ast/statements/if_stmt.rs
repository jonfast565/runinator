#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct IfStmt {
    /// each arm is a (condition, body); the first is `if`, the rest are `else if`.
    pub arms: Vec<(Cond, Block)>,
    pub else_block: Option<Block>,
}
