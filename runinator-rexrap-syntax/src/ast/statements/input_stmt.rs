#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct InputStmt {
    pub prompt: Option<Expr>,
}
