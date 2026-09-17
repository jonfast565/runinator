#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct SignalStmt {
    pub name: String,
    /// `key <expr>`: a correlation value resolved at park time so external webhooks can route here.
    pub correlation_key: Option<Expr>,
    pub metadata: Vec<(String, Expr)>,
}
