#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub struct NoopOutbox;

impl ResultOutbox for NoopOutbox {
    fn append_effect(&self, _message: EffectResultMessage) -> Result<(), OutboxError> {
        Err(OutboxError::Disabled)
    }

    fn next(&self) -> Result<Option<OutboxEntry>, OutboxError> {
        Ok(None)
    }

    fn acknowledge(&self, _id: Uuid) -> Result<(), OutboxError> {
        Ok(())
    }

    fn record_failure(&self, _id: Uuid, _error: String) -> Result<(), OutboxError> {
        Ok(())
    }

    fn depth(&self) -> u64 {
        0
    }

    fn is_full(&self) -> bool {
        false
    }
}
