use crate::{
    BrokerDelivery, BrokerMessage, ConsumerProfile, ControlCommand, ControlDelivery, EventDelivery,
    EventMessage, IngressDelivery, IngressMessage, ResultDelivery, ResultMessage, WakeDelivery,
    WakeMessage,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TcpRequest {
    Publish { message: BrokerMessage },
    PublishControl { command: ControlCommand },
    PublishResult { message: ResultMessage },
    PublishWake { message: WakeMessage },
    PublishIngress { message: IngressMessage },
    PublishEvent { message: EventMessage },
    Receive { consumer: String },
    ReceiveFor { profile: ConsumerProfile },
    ReceiveControl { consumer: String },
    ReceiveControlFor { profile: ConsumerProfile },
    ReceiveResult { consumer: String },
    ReceiveWake { consumer: String },
    ReceiveIngress { consumer: String },
    ReceiveEvent { consumer: String },
    Ack { consumer: String, delivery_id: Uuid },
    AckControl { consumer: String, delivery_id: Uuid },
    AckResult { consumer: String, delivery_id: Uuid },
    AckWake { consumer: String, delivery_id: Uuid },
    AckIngress { consumer: String, delivery_id: Uuid },
    Nack { consumer: String, delivery_id: Uuid },
    NackControl { consumer: String, delivery_id: Uuid },
    NackResult { consumer: String, delivery_id: Uuid },
    NackWake { consumer: String, delivery_id: Uuid },
    NackIngress { consumer: String, delivery_id: Uuid },
}

impl TcpRequest {
    /// the operation's wire tag, for logs and for refusal messages that need to name what was
    /// refused. matched exhaustively on purpose: a new channel should not be able to slip into the
    /// wire protocol without someone deciding what this — and the relay's allow-list — says about it.
    pub fn operation_name(&self) -> &'static str {
        match self {
            Self::Publish { .. } => "publish",
            Self::PublishControl { .. } => "publish_control",
            Self::PublishResult { .. } => "publish_result",
            Self::PublishWake { .. } => "publish_wake",
            Self::PublishIngress { .. } => "publish_ingress",
            Self::PublishEvent { .. } => "publish_event",
            Self::Receive { .. } => "receive",
            Self::ReceiveFor { .. } => "receive_for",
            Self::ReceiveControl { .. } => "receive_control",
            Self::ReceiveControlFor { .. } => "receive_control_for",
            Self::ReceiveResult { .. } => "receive_result",
            Self::ReceiveWake { .. } => "receive_wake",
            Self::ReceiveIngress { .. } => "receive_ingress",
            Self::ReceiveEvent { .. } => "receive_event",
            Self::Ack { .. } => "ack",
            Self::AckControl { .. } => "ack_control",
            Self::AckResult { .. } => "ack_result",
            Self::AckWake { .. } => "ack_wake",
            Self::AckIngress { .. } => "ack_ingress",
            Self::Nack { .. } => "nack",
            Self::NackControl { .. } => "nack_control",
            Self::NackResult { .. } => "nack_result",
            Self::NackWake { .. } => "nack_wake",
            Self::NackIngress { .. } => "nack_ingress",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TcpResponse {
    Ok,
    Delivery { delivery: BrokerDelivery },
    ControlDelivery { delivery: ControlDelivery },
    ResultDelivery { delivery: ResultDelivery },
    WakeDelivery { delivery: WakeDelivery },
    IngressDelivery { delivery: IngressDelivery },
    EventDelivery { delivery: EventDelivery },
    Error { message: String },
}
