#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigStmt {
    pub name: Option<Expr>,
    pub metadata: Option<Expr>,
}
