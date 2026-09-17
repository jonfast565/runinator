#[allow(unused_imports)]
use super::*;

/// `collect "name" max <n> (timeout <dur>)?`: a timed accumulator.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectStmt {
    pub name: String,
    pub max: i64,
    pub timeout: Option<i64>,
}
