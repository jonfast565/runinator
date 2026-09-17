#[allow(unused_imports)]
use super::*;

/// an interrupt asked for from outside the run, waiting for the next drive of its target thread.
///
/// requested sources cannot be a predicate over node state — nothing about the run changed when the
/// caller asked — so the ask is parked here and the ordinary raise path picks it up. it is consumed
/// by the drive that decides about it, raised or refused, so there is no ghost request that can fire
/// at an arbitrary later point in the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingInterrupt {
    #[serde(default = "Uuid::now_v7")]
    pub id: Uuid,
    #[serde(default)]
    pub source: InterruptSource,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
    /// the thread to interrupt. `None` lets whichever real cursor drives next take it, which is what
    /// a run-scoped ask (an orphan signal) wants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<Uuid>,
    #[serde(default = "Utc::now")]
    pub requested_at: DateTime<Utc>,
}

impl PendingInterrupt {
    pub fn new(source: InterruptSource, payload: Value, cursor_id: Option<Uuid>) -> Self {
        Self {
            id: Uuid::now_v7(),
            source,
            payload,
            cursor_id,
            requested_at: Utc::now(),
        }
    }

    /// may this request be raised on `cursor_id`? an untargeted request is for any real thread.
    pub fn targets(&self, cursor_id: Uuid) -> bool {
        self.cursor_id.is_none_or(|target| target == cursor_id)
    }
}
