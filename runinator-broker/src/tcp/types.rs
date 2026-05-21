use crate::{BrokerDelivery, BrokerMessage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TcpRequest {
    Publish { message: BrokerMessage },
    Receive { consumer: String },
    Ack { consumer: String, delivery_id: Uuid },
    Nack { consumer: String, delivery_id: Uuid },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TcpResponse {
    Ok,
    Delivery { delivery: BrokerDelivery },
    Error { message: String },
}
