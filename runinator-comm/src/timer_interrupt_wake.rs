#[allow(unused_imports)]
use super::*;

/// A due occurrence of one configured workflow timer. The timer id names a frozen interrupt
/// declaration, so several independent periods can coexist on the same run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerInterruptWake {
    pub workflow_run_id: Uuid,
    pub timer_id: String,
    pub interval_seconds: i64,
}
