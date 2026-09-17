#[allow(unused_imports)]
use super::*;

/// `mutex "name" (every <dur>)? (timeout <dur>)? (hold <dur>)? ({ body })?` or the bare release leaf
/// `mutex release "name"`: a named cross-run exclusive lock. `timeout` bounds the wait-to-acquire,
/// `hold` marks the expected maximum section duration without displacing an active holder, and a
/// `body` block brackets a critical section that releases at its end.
#[derive(Debug, Clone, PartialEq)]
pub struct MutexStmt {
    pub name: String,
    pub poll_interval: Option<i64>,
    pub timeout: Option<i64>,
    pub hold: Option<i64>,
    /// true when this is a release leaf (`mutex release "name"`); it takes no other clauses or body.
    pub release: bool,
    /// critical-section body; empty for an acquire-only leaf or a release leaf.
    pub body: Vec<Stmt>,
}
