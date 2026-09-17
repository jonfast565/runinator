use chrono::{DateTime, Utc};
use runinator_comm::{
    AgentCommand, ControlCommand, EffectCommand, EffectExecutor, EffectResult, UiEvent,
    WakeCommand, WsIngressCommand,
};
use runinator_models::workflow_vm::{WorkflowEffectRequest, DEFAULT_ACTION_TIMEOUT_SECONDS};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const LEGACY_ACTION_DEADLINE_GRACE_SECONDS: i64 = 30;

fn effect_expires_at(
    command: &EffectCommand,
    enqueued_at: DateTime<Utc>,
    explicit: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    if explicit.is_some() {
        return explicit;
    }
    // Notification actions have no workflow deadline wake to settle them after a silent broker
    // expiry. Infrastructure effects own their lifetime in their request (some intentionally park
    // indefinitely), so only ordinary provider actions receive the compatibility fallback.
    if command.executor != EffectExecutor::Provider || command.notification_delivery_id.is_some() {
        return None;
    }
    let WorkflowEffectRequest::Action {
        timeout_seconds, ..
    } = &command.request
    else {
        return None;
    };
    let budget = timeout_seconds
        .unwrap_or(DEFAULT_ACTION_TIMEOUT_SECONDS)
        .max(1);
    Some(enqueued_at + chrono::Duration::seconds(budget + LEGACY_ACTION_DEADLINE_GRACE_SECONDS))
}

/// where a self-reconnecting transport currently stands with its backend.
///
/// only transports that own a long-lived connection and re-establish it themselves report this (the
/// `WS` relay today); see [`crate::Broker::connection_state`]. it exists so a host can *show* the
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

mod control_delivery;
pub use control_delivery::ControlDelivery;

mod agent_delivery;
pub use agent_delivery::AgentDelivery;

mod effect_message;
pub use effect_message::EffectMessage;

mod effect_delivery;
pub use effect_delivery::EffectDelivery;

mod effect_result_message;
pub use effect_result_message::EffectResultMessage;

mod effect_result_delivery;
pub use effect_result_delivery::EffectResultDelivery;

mod wake_message;
pub use wake_message::WakeMessage;

mod wake_delivery;
pub use wake_delivery::WakeDelivery;

mod ingress_message;
pub use ingress_message::IngressMessage;

mod ingress_delivery;
pub use ingress_delivery::IngressDelivery;

mod event_message;
pub use event_message::EventMessage;

mod event_delivery;
pub use event_delivery::EventDelivery;
