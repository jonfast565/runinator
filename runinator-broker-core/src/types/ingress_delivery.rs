#[allow(unused_imports)]
use super::*;

/// Ingress delivery returned when polling the ingress channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressDelivery {
    pub delivery_id: Uuid,
    pub command: WsIngressCommand,
    pub dedupe_key: String,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl From<IngressMessage> for IngressDelivery {
    fn from(message: IngressMessage) -> Self {
        let dedupe = message.dedupe_key_or_hash();
        Self {
            delivery_id: Uuid::new_v4(),
            dedupe_key: dedupe,
            enqueued_at: message.enqueued_at,
            command: message.command,
        }
    }
}
