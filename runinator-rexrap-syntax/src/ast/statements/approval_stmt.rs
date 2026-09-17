#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalStmt {
    pub approval_type: Option<String>,
    pub prompt: Expr,
    pub metadata: Vec<(String, Expr)>,
}
