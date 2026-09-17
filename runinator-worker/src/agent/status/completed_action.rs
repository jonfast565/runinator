#[allow(unused_imports)]
use super::*;

/// a single finished action, kept so a host can show what this machine last did.
#[derive(Debug, Clone)]
pub struct CompletedAction {
    pub summary: String,
    pub outcome: ActionOutcome,
    pub duration_ms: i64,
}
