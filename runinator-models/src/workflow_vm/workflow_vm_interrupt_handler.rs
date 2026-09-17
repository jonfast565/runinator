#[allow(unused_imports)]
use super::*;

/// One compiled interrupt handler target. The source is part of bytecode rather than a lookup in
/// mutable workflow metadata, so a run remains reproducible after its definition changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowVmInterruptHandler {
    pub source: InterruptSource,
    pub target: usize,
    /// Stable identity of a periodic timer declaration. Only set for [`InterruptSource::Timer`].
    /// It is distinct even when two schedules use the same handler region; the timer relay carries
    /// it in its pending-interrupt payload, so multiple timers are never ambiguous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_id: Option<String>,
    /// The frozen period for a periodic timer handler, in seconds. Only set for timer handlers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_seconds: Option<i64>,
}
