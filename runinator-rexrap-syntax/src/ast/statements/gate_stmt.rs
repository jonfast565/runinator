#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct GateStmt {
    pub kind: String,
    pub when: Option<Cond>,
    pub poll_interval: Option<i64>,
    pub timeout: Option<i64>,
    pub timeout_policy: Option<String>,
    pub metadata: Vec<(String, Expr)>,
}
