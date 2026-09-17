#[allow(unused_imports)]
use super::*;

/// a UI event published on the broker fan-out `events` channel. best-effort: no dedupe, no ack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMessage {
    pub event: UiEvent,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl EventMessage {
    pub fn new(event: UiEvent) -> Self {
        Self {
            event,
            enqueued_at: utc_now(),
        }
    }
}
