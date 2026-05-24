use crate::{
    BrokerDelivery, BrokerMessage, ControlCommand, ControlDelivery, ResultDelivery, ResultMessage,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TcpRequest {
    Publish { message: BrokerMessage },
    PublishControl { command: ControlCommand },
    PublishResult { message: ResultMessage },
    Receive { consumer: String },
    ReceiveControl { consumer: String },
    ReceiveResult { consumer: String },
    Ack { consumer: String, delivery_id: Uuid },
    AckControl { consumer: String, delivery_id: Uuid },
    AckResult { consumer: String, delivery_id: Uuid },
    Nack { consumer: String, delivery_id: Uuid },
    NackResult { consumer: String, delivery_id: Uuid },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TcpResponse {
    Ok,
    Delivery { delivery: BrokerDelivery },
    ControlDelivery { delivery: ControlDelivery },
    ResultDelivery { delivery: ResultDelivery },
    Error { message: String },
}
