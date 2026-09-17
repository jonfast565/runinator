#[allow(unused_imports)]
use super::*;

/// a UI event delivery handed to one fan-out subscriber. every subscriber receives its own copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDelivery {
    pub delivery_id: Uuid,
    pub event: UiEvent,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl From<EventMessage> for EventDelivery {
    fn from(message: EventMessage) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            event: message.event,
            enqueued_at: message.enqueued_at,
        }
    }
}
