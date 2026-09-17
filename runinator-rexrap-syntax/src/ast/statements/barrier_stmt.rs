#[allow(unused_imports)]
use super::*;

/// `barrier "name" count <n> ...`: a multi-run rendezvous.
#[derive(Debug, Clone, PartialEq)]
pub struct BarrierStmt {
    pub name: String,
    pub count: i64,
    pub poll_interval: Option<i64>,
    pub timeout: Option<i64>,
}
