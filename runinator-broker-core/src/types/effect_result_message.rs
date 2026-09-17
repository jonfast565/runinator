#[allow(unused_imports)]
use super::*;

/// A VM effect result queued for the durable VM host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectResultMessage {
    pub result: EffectResult,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl EffectResultMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.result.event_id.to_string())
    }
}
