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
#[serde(tag = "outcome", content = "record", rename_all = "snake_case")]
pub enum BrokerIngressCapture {
    Observed(BrokerIngressRecord),
    Held(BrokerIngressRecord),
    Duplicate(BrokerIngressRecord),
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerMessageDirection {
    Published,
    Received,
}

mod external_ingress_gate;
pub use external_ingress_gate::ExternalIngressGate;

mod external_ingress_record;
pub use external_ingress_record::ExternalIngressRecord;

mod broker_ingress_session;
pub use broker_ingress_session::BrokerIngressSession;

mod broker_ingress_capture_request;
pub use broker_ingress_capture_request::BrokerIngressCaptureRequest;

mod broker_ingress_record;
pub use broker_ingress_record::BrokerIngressRecord;

mod broker_message_record;
pub use broker_message_record::BrokerMessageRecord;

mod ingress_control_query;
pub use ingress_control_query::IngressControlQuery;

mod external_ingress_capture_request;
pub use external_ingress_capture_request::ExternalIngressCaptureRequest;
