#[allow(unused_imports)]
use super::*;

/// `event_source type <str> (filter <cond>)? (max <n>)? (timeout <dur>)?`: stream-driven iteration.
#[derive(Debug, Clone, PartialEq)]
pub struct EventSourceStmt {
    pub event_type: String,
    pub filter: Option<Cond>,
    pub max: Option<i64>,
    pub timeout: Option<i64>,
}
