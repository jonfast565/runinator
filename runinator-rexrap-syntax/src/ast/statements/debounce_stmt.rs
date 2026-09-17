#[allow(unused_imports)]
use super::*;

/// `debounce "name" delay <dur> (key <expr>)?`: a trailing-delay window with external reset.
#[derive(Debug, Clone, PartialEq)]
pub struct DebounceStmt {
    pub name: String,
    pub delay_seconds: i64,
    pub key: Option<Expr>,
}
