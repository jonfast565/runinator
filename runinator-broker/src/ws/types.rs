//! Wire envelope for the `WS` transport. TCP opens a connection for each RPC, while HTTP sends one
//! request at a time. WS keeps one two-way connection, so each response needs a request ID.
//! `WsFrame` adds that ID around the existing TCP request and response payloads. Both transports
//! then use the same message enums.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::tcp::types::{TcpRequest, TcpResponse};

#[derive(Debug, Serialize, Deserialize)]
pub struct WsFrame<T> {
    pub request_id: Uuid,
    #[serde(flatten)]
    pub body: T,
}

impl<T> WsFrame<T> {
    pub fn new(request_id: Uuid, body: T) -> Self {
        Self { request_id, body }
    }
}

pub type WsRequestFrame = WsFrame<TcpRequest>;
pub type WsResponseFrame = WsFrame<TcpResponse>;
