#[allow(unused_imports)]
use super::*;

pub trait ResultOutbox: Send + Sync {
    /// Fsync a terminal status or artifact before its effect delivery may be acknowledged.
    fn append_effect(&self, message: EffectResultMessage) -> Result<(), OutboxError>;
    fn next(&self) -> Result<Option<OutboxEntry>, OutboxError>;
    fn acknowledge(&self, id: Uuid) -> Result<(), OutboxError>;
    fn record_failure(&self, id: Uuid, error: String) -> Result<(), OutboxError>;
    fn depth(&self) -> u64;
    fn is_full(&self) -> bool;
}
