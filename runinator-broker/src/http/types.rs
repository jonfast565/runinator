use crate::{BrokerDelivery, BrokerMessage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishRequest {
    pub message: BrokerMessage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PollRequest {
    pub consumer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PollResponse {
    pub delivery: Option<BrokerDelivery>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AckRequest {
    pub consumer: String,
    pub delivery_id: Uuid,
}
