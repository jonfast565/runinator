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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressControlQuery {
    pub scope: Option<ScopeRef>,
    pub target_kind: Option<IngressTargetKind>,
    pub target_id: Option<Uuid>,
    pub state: Option<IngressControlState>,
    pub limit: i64,
}
