//! wire envelope for the `ws` transport: unlike `tcp` (fresh connection per RPC) and `http` (one POST
//! per RPC), a websocket connection is persistent and bidirectional, so concurrent calls sharing one
//! connection need a way to match a response back to its request. `WsFrame` adds just that — a
//! `request_id` — around the existing [`crate::tcp::types::TcpRequest`]/[`crate::tcp::types::TcpResponse`]
//! payloads, rather than duplicating a parallel enum: the tcp and ws transports then stay provably in
//! lockstep (a new channel only ever needs one enum updated, not two).

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
