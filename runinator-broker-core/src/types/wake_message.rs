#[allow(unused_imports)]
use super::*;

/// Wake event queued for waker delivery (delayed reducer drive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeMessage {
    pub command: WakeCommand,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl WakeMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.command.dedupe_key())
    }
}
