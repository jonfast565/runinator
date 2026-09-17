#[allow(unused_imports)]
use super::*;

/// `throttle "name" rate <n> per <dur> ...`: a named cross-run rate limiter.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrottleStmt {
    pub name: String,
    pub max_per_window: i64,
    pub window_seconds: i64,
    pub poll_interval: Option<i64>,
    pub timeout: Option<i64>,
}
