#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MapStmt {
    pub var: String,
    pub items: Expr,
    pub concurrency: Option<i64>,
    pub body: Block,
}
