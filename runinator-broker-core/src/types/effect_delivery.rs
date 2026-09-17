#[allow(unused_imports)]
use super::*;

/// A leased VM effect command delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectDelivery {
    pub delivery_id: Uuid,
    pub command: EffectCommand,
    pub dedupe_key: String,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl EffectDelivery {
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.effective_expires_at()
            .is_some_and(|expires_at| expires_at <= now)
    }

    pub fn effective_expires_at(&self) -> Option<DateTime<Utc>> {
        effect_expires_at(&self.command, self.enqueued_at, self.expires_at)
    }
}

impl From<EffectMessage> for EffectDelivery {
    fn from(message: EffectMessage) -> Self {
        let expires_at = message.effective_expires_at();
        Self {
            delivery_id: Uuid::new_v4(),
            dedupe_key: message.dedupe_key_or_hash(),
            enqueued_at: message.enqueued_at,
            expires_at,
            command: message.command,
        }
    }
}
