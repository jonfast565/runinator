#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct OutputStmt {
    pub event_type: Option<String>,
    pub data: Option<Expr>,
    /// artifact declarations from `name = expr` lines in the output block.
    pub items: Vec<(String, Expr)>,
}
