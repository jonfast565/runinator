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
