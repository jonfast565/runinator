//! Durable operator controls for external and broker ingress.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    orchestration::{IngressEvent, IngressTarget, IngressTargetKind},
    rbac::ScopeRef,
    value::Value,
};

pub const INGRESS_CONTROL_QUEUE_CAPACITY: i64 = 100;
/// A browser-owned broker-inspection session must renew before this lease expires. The short
/// timeout makes a disconnected or closed inspector fail safe without leaving traffic captured.
pub const BROKER_INGRESS_SESSION_TTL_SECONDS: i64 = 15;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalIngressGateMode {
    #[default]
    Disabled,
    Paused,
    Review,
}

impl ExternalIngressGateMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Paused => "paused",
            Self::Review => "review",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalIngressGate {
    pub target: IngressTarget,
    pub owner_scope: ScopeRef,
    pub mode: ExternalIngressGateMode,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressControlState {
    Held,
    Approved,
    Applying,
    Applied,
    Dropped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalIngressRecord {
    pub id: Uuid,
    pub target: IngressTarget,
    pub owner_scope: ScopeRef,
    pub gate_mode: ExternalIngressGateMode,
    pub event: IngressEvent,
    pub state: IngressControlState,
    pub queue_position: Option<i64>,
    pub reviewed_by: Option<Uuid>,
    pub last_error: Option<String>,
    pub received_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "record", rename_all = "snake_case")]
pub enum ExternalIngressCapture {
    Stored(ExternalIngressRecord),
    Duplicate(ExternalIngressRecord),
    Full,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerIngressSessionMode {
    #[default]
    Off,
    Observe,
    HoldOrchestrationNudges,
}

impl BrokerIngressSessionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::HoldOrchestrationNudges => "hold_orchestration_nudges",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerIngressSession {
    pub scope: ScopeRef,
    pub mode: BrokerIngressSessionMode,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    /// An inspector is a client-owned, renewable lease rather than a sticky server setting. This
    /// lets the engine stop recording/holding ingress shortly after the inspecting page closes or
    /// loses its connection.
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerIngressCaptureRequest {
    pub scope: ScopeRef,
    pub delivery_id: Uuid,
    pub dedupe_key: String,
    pub command_kind: String,
    pub command: Value,
    pub hold: bool,
    pub received_at: DateTime<Utc>,
    pub capacity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerIngressRecord {
    pub id: Uuid,
    pub scope: ScopeRef,
    pub delivery_id: Uuid,
    pub dedupe_key: String,
    pub command_kind: String,
    pub command: Value,
    pub state: IngressControlState,
    pub reviewed_by: Option<Uuid>,
    pub last_error: Option<String>,
    pub received_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "record", rename_all = "snake_case")]
pub enum BrokerIngressCapture {
    Observed(BrokerIngressRecord),
    Held(BrokerIngressRecord),
    Duplicate(BrokerIngressRecord),
    Full,
}

/// One engine-side observation of a message crossing a broker channel.
///
/// The trace deliberately records the broker envelope at the engine boundary. It is not a queue
/// inspector: worker-local receives remain local, while every message that enters or leaves the
/// durable engine is available to the workflow or pipeline run that owns it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerMessageRecord {
    pub id: Uuid,
    /// `effect`, `effect_result`, `wake`, `ingress`, `control`, or `agent`.
    pub channel: String,
    /// `published` when the engine wrote the message, `received` when it accepted a delivery.
    pub direction: BrokerMessageDirection,
    /// The typed broker payload carried on the channel, such as `effect_command`.
    pub message_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<Uuid>,
    pub payload: Value,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerMessageDirection {
    Published,
    Received,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressControlQuery {
    pub scope: Option<ScopeRef>,
    pub target_kind: Option<IngressTargetKind>,
    pub target_id: Option<Uuid>,
    pub state: Option<IngressControlState>,
    pub limit: i64,
}
