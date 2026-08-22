use chrono::{DateTime, Utc};
use runinator_comm::{
    AgentCommand, ControlCommand, EffectCommand, EffectResult, UiEvent, WakeCommand,
    WsIngressCommand,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlDelivery {
    pub delivery_id: Uuid,
    pub command: ControlCommand,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDelivery {
    pub delivery_id: Uuid,
    pub command: AgentCommand,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// A VM effect command queued for a provider worker or an infrastructure effect host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectMessage {
    pub command: EffectCommand,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// A leased VM effect command delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectDelivery {
    pub delivery_id: Uuid,
    pub command: EffectCommand,
    pub dedupe_key: String,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// A VM effect result queued for the durable VM host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectResultMessage {
    pub result: EffectResult,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

/// A leased VM effect result delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectResultDelivery {
    pub delivery_id: Uuid,
    pub result: EffectResult,
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

impl EffectMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.command.effect_id.to_string())
    }
}

impl EffectResultMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.result.event_id.to_string())
    }
}

impl From<EffectMessage> for EffectDelivery {
    fn from(message: EffectMessage) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            dedupe_key: message.dedupe_key_or_hash(),
            enqueued_at: message.enqueued_at,
            command: message.command,
        }
    }
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

impl From<ControlCommand> for ControlDelivery {
    fn from(command: ControlCommand) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            command,
            enqueued_at: utc_now(),
        }
    }
}

impl From<AgentCommand> for AgentDelivery {
    fn from(command: AgentCommand) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            command,
            enqueued_at: utc_now(),
        }
    }
}

/// where a self-reconnecting transport currently stands with its backend.
///
/// only transports that own a long-lived connection and re-establish it themselves report this (the
/// `ws` relay today); see [`crate::Broker::connection_state`]. it exists so a host can *show* the
/// difference between "idle and healthy" and "silently retrying for the last ten minutes" — which
/// otherwise only appears in logs, and which is the normal condition for an agent behind NAT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionState {
    /// no connection attempted yet, or the transport has been shut down.
    Idle,
    /// an attempt is in flight; nothing has been established yet.
    Connecting,
    /// a connection is live and requests are being served on it.
    Connected,
    /// the last connection failed or dropped and another attempt is scheduled.
    Reconnecting { retry_secs: u64, reason: String },
    /// the backend rejected our credential. retrying cannot fix this, so a host should surface it
    /// rather than let the transport reconnect-loop against a credential that will never be accepted.
    Unauthorized { reason: String },
}

impl ConnectionState {
    /// whether requests can currently be served. `false` for every non-[`Self::Connected`] state.
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    /// whether this state can only be cleared by operator action (re-enrollment, a new key) rather
    /// than by waiting.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Unauthorized { .. })
    }
}

fn utc_now() -> DateTime<Utc> {
    Utc::now()
}
