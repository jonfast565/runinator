//! A standalone `WS` broker server. It binds, accepts connections, and dispatches each request to a
//! shared `Broker`. Unlike TCP, the connection is long-lived and multiplexed. Each request gets its
//! own task, so a slow `receive_for` does not block a later `ack` on the same connection.
//!
//! The WS transport tests connect a [`crate::ws::WsBroker`] client to this server.
//! The real `runinator-ws` relay uses its own Axum router for authentication, but it uses the same
//! frame types and dispatch function. The two hosts therefore share the same wire format.

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
