use chrono::{DateTime, Utc};
use runinator_comm::TaskCommand;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

/// Payload delivered through the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerMessage {
    pub command: TaskCommand,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// Message returned when polling the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerDelivery {
    pub delivery_id: Uuid,
    pub command: TaskCommand,
    pub dedupe_key: String,
    pub enqueued_at: DateTime<Utc>,
}

impl BrokerMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key.clone().unwrap_or_else(|| {
            let mut hasher = DefaultHasher::new();
            if let Ok(serialized) = serde_json::to_string(&self.command) {
                serialized.hash(&mut hasher);
            } else {
                self.command.command_id.hash(&mut hasher);
            }
            format!("{:x}", hasher.finish())
        })
    }
}

impl From<BrokerMessage> for BrokerDelivery {
    fn from(message: BrokerMessage) -> Self {
        let dedupe = message.dedupe_key_or_hash();
        Self {
            delivery_id: Uuid::new_v4(),
            dedupe_key: dedupe,
            enqueued_at: message.enqueued_at,
            command: message.command,
        }
    }
}

fn utc_now() -> DateTime<Utc> {
    Utc::now()
}
