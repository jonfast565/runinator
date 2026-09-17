#[allow(unused_imports)]
use super::*;

/// Ingress message queued for web-service consumption (drive / control request).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressMessage {
    pub command: WsIngressCommand,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl IngressMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.command.dedupe_key())
    }
}
