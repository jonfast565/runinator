use crate::{
    AgentCommand, AgentDelivery, ConsumerProfile, ControlCommand, ControlDelivery, EffectDelivery,
    EffectMessage, EffectResultDelivery, EffectResultMessage, EventDelivery, EventMessage,
    IngressDelivery, IngressMessage, WakeDelivery, WakeMessage,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TcpRequest {
    Heartbeat,
    PublishControl { command: ControlCommand },
    PublishAgent { command: AgentCommand },
    PublishEffect { message: Box<EffectMessage> },
    PublishEffectResult { message: EffectResultMessage },
    PublishWake { message: WakeMessage },
    PublishIngress { message: IngressMessage },
    PublishEvent { message: EventMessage },
    ReceiveControl { consumer: String },
    ReceiveControlFor { profile: ConsumerProfile },
    ReceiveAgent { consumer: String },
    ReceiveAgentFor { profile: ConsumerProfile },
    ReceiveEffect { consumer: String },
    ReceiveEffectFor { profile: ConsumerProfile },
    ReceiveInfrastructureEffect { consumer: String },
    ReceiveEffectResult { consumer: String },
    ReceiveWake { consumer: String },
    ReceiveIngress { consumer: String },
    ReceiveEvent { consumer: String },
    AckControl { consumer: String, delivery_id: Uuid },
    AckAgent { consumer: String, delivery_id: Uuid },
    AckEffect { consumer: String, delivery_id: Uuid },
    AckEffectResult { consumer: String, delivery_id: Uuid },
    AckWake { consumer: String, delivery_id: Uuid },
    AckIngress { consumer: String, delivery_id: Uuid },
    NackControl { consumer: String, delivery_id: Uuid },
    NackAgent { consumer: String, delivery_id: Uuid },
    NackEffect { consumer: String, delivery_id: Uuid },
    NackEffectResult { consumer: String, delivery_id: Uuid },
    NackWake { consumer: String, delivery_id: Uuid },
    NackIngress { consumer: String, delivery_id: Uuid },
}

impl TcpRequest {
    /// the operation's wire tag, for logs and for refusal messages that need to name what was
    /// refused. matched exhaustively on purpose: a new channel should not be able to slip into the
    /// wire protocol without someone deciding what this — and the relay's allow-list — says about it.
    pub fn operation_name(&self) -> &'static str {
        match self {
            Self::Heartbeat => "heartbeat",
            Self::PublishControl { .. } => "publish_control",
            Self::PublishAgent { .. } => "publish_agent",
            Self::PublishEffect { .. } => "publish_effect",
            Self::PublishEffectResult { .. } => "publish_effect_result",
            Self::PublishWake { .. } => "publish_wake",
            Self::PublishIngress { .. } => "publish_ingress",
            Self::PublishEvent { .. } => "publish_event",
            Self::ReceiveControl { .. } => "receive_control",
            Self::ReceiveControlFor { .. } => "receive_control_for",
            Self::ReceiveAgent { .. } => "receive_agent",
            Self::ReceiveAgentFor { .. } => "receive_agent_for",
            Self::ReceiveEffect { .. } => "receive_effect",
            Self::ReceiveEffectFor { .. } => "receive_effect_for",
            Self::ReceiveInfrastructureEffect { .. } => "receive_infrastructure_effect",
            Self::ReceiveEffectResult { .. } => "receive_effect_result",
            Self::ReceiveWake { .. } => "receive_wake",
            Self::ReceiveIngress { .. } => "receive_ingress",
            Self::ReceiveEvent { .. } => "receive_event",
            Self::AckControl { .. } => "ack_control",
            Self::AckAgent { .. } => "ack_agent",
            Self::AckEffect { .. } => "ack_effect",
            Self::AckEffectResult { .. } => "ack_effect_result",
            Self::AckWake { .. } => "ack_wake",
            Self::AckIngress { .. } => "ack_ingress",
            Self::NackControl { .. } => "nack_control",
            Self::NackAgent { .. } => "nack_agent",
            Self::NackEffect { .. } => "nack_effect",
            Self::NackEffectResult { .. } => "nack_effect_result",
            Self::NackWake { .. } => "nack_wake",
            Self::NackIngress { .. } => "nack_ingress",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TcpResponse {
    Ok,
    ControlDelivery { delivery: ControlDelivery },
    AgentDelivery { delivery: AgentDelivery },
    EffectDelivery { delivery: Box<EffectDelivery> },
    EffectResultDelivery { delivery: EffectResultDelivery },
    WakeDelivery { delivery: WakeDelivery },
    IngressDelivery { delivery: IngressDelivery },
    EventDelivery { delivery: EventDelivery },
    Error { message: String },
}
