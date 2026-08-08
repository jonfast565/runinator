use chrono::{DateTime, Utc};
use runinator_comm::{
    ActionCommand, ControlCommand, UiEvent, WakeCommand, WorkflowResultEvent, WsIngressCommand,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

/// Payload delivered through the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerMessage {
    pub command: ActionCommand,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// Message returned when polling the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerDelivery {
    pub delivery_id: Uuid,
    pub command: ActionCommand,
    pub dedupe_key: String,
    pub enqueued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlDelivery {
    pub delivery_id: Uuid,
    pub command: ControlCommand,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// Result event queued for web-service persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultMessage {
    pub event: WorkflowResultEvent,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// Result event delivery returned when polling the result channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultDelivery {
    pub delivery_id: Uuid,
    pub event: WorkflowResultEvent,
    pub dedupe_key: String,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// Wake event queued for waker delivery (delayed reducer drive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeMessage {
    pub command: WakeCommand,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// Wake delivery returned when polling the wake channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeDelivery {
    pub delivery_id: Uuid,
    pub command: WakeCommand,
    pub dedupe_key: String,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// Ingress message queued for web-service consumption (drive / control request).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressMessage {
    pub command: WsIngressCommand,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// Ingress delivery returned when polling the ingress channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressDelivery {
    pub delivery_id: Uuid,
    pub command: WsIngressCommand,
    pub dedupe_key: String,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// a UI event published on the broker fan-out `events` channel. best-effort: no dedupe, no ack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMessage {
    pub event: UiEvent,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// a UI event delivery handed to one fan-out subscriber. every subscriber receives its own copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDelivery {
    pub delivery_id: Uuid,
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

impl From<EventMessage> for EventDelivery {
    fn from(message: EventMessage) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            event: message.event,
            enqueued_at: message.enqueued_at,
        }
    }
}

impl WakeMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.command.dedupe_key())
    }
}

impl IngressMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.command.dedupe_key())
    }
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

impl ResultMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.event.event_id.to_string())
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

impl From<ControlCommand> for ControlDelivery {
    fn from(command: ControlCommand) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            command,
            enqueued_at: utc_now(),
        }
    }
}

impl From<ResultMessage> for ResultDelivery {
    fn from(message: ResultMessage) -> Self {
        let dedupe = message.dedupe_key_or_hash();
        Self {
            delivery_id: Uuid::new_v4(),
            dedupe_key: dedupe,
            enqueued_at: message.enqueued_at,
            event: message.event,
        }
    }
}

fn utc_now() -> DateTime<Utc> {
    Utc::now()
}
