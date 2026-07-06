//! a standalone `ws` broker server, mirroring `tcp::server`'s shape: bind, accept, and for each
//! connection dispatch every incoming request against a shared `Broker`. unlike `tcp` (one request
//! per connection) this is long-lived and multiplexed — each request is dispatched on its own spawned
//! task so a slow `receive_for` never blocks a concurrent `ack` arriving moments later on the same
//! connection.
//!
//! this is what `runinator-broker/tests/ws.rs` connects a [`crate::ws::WsBroker`] client against.
//! `runinator-ws`'s real relay endpoint hosts the same wire protocol directly on its own axum router
//! (to inherit its auth middleware and `Extension<Arc<dyn Broker>>`) rather than embedding this
//! server, since axum's `WebSocket` and this module's raw `tokio-tungstenite` socket are different
//! types — but both decode/encode the exact same [`crate::ws::types::WsFrame`] and call the exact
//! same [`crate::dispatch::dispatch`], so the two hosts can never drift on wire format or semantics.

use std::{net::SocketAddr, sync::Arc};

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    dispatch::dispatch,
    ws::types::{WsRequestFrame, WsResponseFrame},
    Broker,
};

pub async fn run_server<B>(addr: SocketAddr, broker: B) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let listener = TcpListener::bind(addr).await?;
    serve(listener, broker).await
}

pub async fn serve<B>(listener: TcpListener, broker: B) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let broker = Arc::new(broker);
    loop {
        let (stream, _) = listener.accept().await?;
        let broker = Arc::clone(&broker);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, broker).await {
                log::warn!("broker ws connection error: {err}");
            }
        });
    }
}

async fn handle_connection<B>(
    stream: TcpStream,
    broker: Arc<B>,
) -> Result<(), tokio_tungstenite::tungstenite::Error>
where
    B: Broker,
{
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    let (sink, mut source) = ws_stream.split();
    let sink = Arc::new(tokio::sync::Mutex::new(sink));

    while let Some(next) = source.next().await {
        let Message::Text(text) = next? else {
            continue;
        };
        let Ok(frame) = serde_json::from_str::<WsRequestFrame>(&text) else {
            continue;
        };
        let broker = Arc::clone(&broker);
        let sink = Arc::clone(&sink);
        // each request gets its own task so a slow `receive_for`/`receive_control` never blocks a
        // concurrent, faster request (e.g. `ack`) arriving on the same connection in the meantime.
        tokio::spawn(async move {
            let response = dispatch(broker.as_ref(), frame.body).await;
            let payload =
                match serde_json::to_string(&WsResponseFrame::new(frame.request_id, response)) {
                    Ok(payload) => payload,
                    Err(_) => return,
                };
            let _ = sink.lock().await.send(Message::Text(payload.into())).await;
        });
    }
    Ok(())
}
