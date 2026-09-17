#[allow(unused_imports)]
use super::*;

/// `audit action <expr> (actor <expr>)? (target <expr>)? (reason <expr>)?`: a compliance record.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditStmt {
    pub action: Expr,
    pub actor: Option<Expr>,
    pub target: Option<Expr>,
    pub reason: Option<Expr>,
}
