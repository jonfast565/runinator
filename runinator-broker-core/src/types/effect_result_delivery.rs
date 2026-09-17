#[allow(unused_imports)]
use super::*;

/// A leased VM effect result delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectResultDelivery {
    pub delivery_id: Uuid,
    pub result: EffectResult,
    pub dedupe_key: String,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl From<EffectResultMessage> for EffectResultDelivery {
    fn from(message: EffectResultMessage) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            dedupe_key: message.dedupe_key_or_hash(),
            enqueued_at: message.enqueued_at,
            result: message.result,
        }
    }
}
