#[allow(unused_imports)]
use super::*;

/// Wake delivery returned when polling the wake channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeDelivery {
    pub delivery_id: Uuid,
    pub command: WakeCommand,
    pub dedupe_key: String,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl From<WakeMessage> for WakeDelivery {
    fn from(message: WakeMessage) -> Self {
        let dedupe = message.dedupe_key_or_hash();
        Self {
            delivery_id: Uuid::new_v4(),
            dedupe_key: dedupe,
            enqueued_at: message.enqueued_at,
            command: message.command,
        }
    }
}
