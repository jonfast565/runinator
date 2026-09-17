#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MatchStmt {
    pub subject: Expr,
    pub mode: SwitchMode,
    pub arms: Vec<MatchArm>,
    pub default: Option<Block>,
}
