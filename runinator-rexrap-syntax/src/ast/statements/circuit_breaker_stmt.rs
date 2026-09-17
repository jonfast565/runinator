#[allow(unused_imports)]
use super::*;

/// `circuit_breaker "name" threshold <n> window <dur> cooldown <dur>`: a cross-run failure guard.
#[derive(Debug, Clone, PartialEq)]
pub struct CircuitBreakerStmt {
    pub name: String,
    pub threshold: i64,
    pub window_seconds: i64,
    pub cooldown_seconds: i64,
}
