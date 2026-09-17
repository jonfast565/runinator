#[allow(unused_imports)]
use super::*;

/// `cooldown "name" every <dur>`: a named cross-run cooldown gate.
#[derive(Debug, Clone, PartialEq)]
pub struct CooldownStmt {
    pub name: String,
    pub window_seconds: i64,
}
