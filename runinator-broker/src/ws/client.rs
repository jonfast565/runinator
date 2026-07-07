//! the `ws` transport client: unlike `TcpBroker` (fresh connection per RPC) or `HttpBroker` (one POST
//! per RPC), a websocket connection is persistent and bidirectional, so this multiplexes every
//! concurrent `Broker` call over one connection using a `request_id`-correlated pending map.
//!
//! connection ownership: a background connector task holds the only live connection and installs a
//! [`ConnectionHandle`] (a writer sender + that connection's pending map) into a shared slot once
//! connected; it clears the slot and reconnects (with backoff+jitter, reset on success) whenever the
//! connection drops. every public method takes a snapshot of the current handle, sends its request,
//! and awaits a oneshot reply — so a long-blocking `receive_for` sitting in the pending map never
//! blocks a concurrent `ack` on the same connection; the reader task's dispatch is an O(1) hashmap
//! lookup independent of arrival order.
//!
//! `receive_for`/`receive_control` retry indefinitely across reconnects (per
//! [`crate::Broker::receive_for`]'s "wait for and retrieve the next delivery" contract, and because
//! `runinator-worker`'s loop treats any error from them as fatal to the whole worker) — a transient
//! disconnect is invisible to the caller, just a longer wait. One-shot ops (`publish`, `ack`, `nack`,
//! ...) retry for a few seconds (long enough to ride out the client's initial connect or a brief
//! reconnect) before surfacing a `BrokerError`, rather than failing on the very next reconnect blip.

use async_trait::async_trait;
use uuid::Uuid;

#[cfg(feature = "ws")]
use crate::ConsumerProfile;
use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, ResultDelivery, ResultMessage,
    WakeDelivery, WakeMessage,
};

#[cfg(feature = "ws")]
mod imp {
    use super::*;
    use crate::tcp::types::{TcpRequest, TcpResponse};
    use crate::ws::reconnect::Backoff;
    use crate::ws::types::{WsRequestFrame, WsResponseFrame};
    use futures_util::{SinkExt, StreamExt};
    use log::warn;
    use parking_lot::Mutex;
    use std::{collections::HashMap, sync::Arc, time::Duration};
    use tokio::sync::{mpsc, oneshot, watch};
    use tokio_tungstenite::tungstenite::Message;

    type PendingMap = Arc<Mutex<HashMap<Uuid, oneshot::Sender<TcpResponse>>>>;

    /// the live connection's send half plus its pending-response map; replaced wholesale on every
    /// reconnect (the old map's outstanding senders are simply dropped, which fails any request still
    /// awaiting a reply on the superseded connection with a retryable error).
    #[derive(Clone)]
    struct ConnectionHandle {
        write_tx: mpsc::UnboundedSender<Message>,
        pending: PendingMap,
    }

    pub struct WsBroker {
        connection: watch::Receiver<Option<ConnectionHandle>>,
        // kept alive for the supervisor task's lifetime; dropping the broker drops this and ends it.
        _supervisor: tokio::task::JoinHandle<()>,
    }

    impl WsBroker {
        /// connect (in the background) to `url` (a `ws://`/`wss://` endpoint), presenting `api_key`
        /// as a bearer token on the upgrade request. returns immediately; the first request made
        /// before the initial connection completes simply waits, same as any later reconnect.
        pub fn connect(url: String, api_key: Option<String>) -> Self {
            let (tx, rx) = watch::channel(None);
            let supervisor = tokio::spawn(run_supervisor(url, api_key, tx));
            Self {
                connection: rx,
                _supervisor: supervisor,
            }
        }

        /// one attempt: send `request` on whatever connection is currently live and await its reply.
        /// returns a retryable `Err` immediately if nothing is connected right now, rather than
        /// waiting — callers that must not give up (`receive_for`/`receive_control`) loop on this.
        async fn request(&self, request: TcpRequest) -> Result<TcpResponse, BrokerError> {
            let handle = self
                .connection
                .borrow()
                .clone()
                .ok_or_else(|| BrokerError::Internal("ws broker: not connected".into()))?;

            let request_id = Uuid::new_v4();
            let (response_tx, response_rx) = oneshot::channel();
            handle.pending.lock().insert(request_id, response_tx);
            // RAII: if this future is dropped before a reply arrives (e.g. a `tokio::select!` losing
            // a race), remove our own entry so it doesn't sit in the map forever.
            let _cleanup = PendingCleanup {
                pending: handle.pending.clone(),
                request_id,
            };

            let frame = WsRequestFrame::new(request_id, request);
            let payload = serde_json::to_string(&frame)
                .map_err(|err| BrokerError::Internal(err.to_string()))?;
            handle
                .write_tx
                .send(Message::Text(payload.into()))
                .map_err(|_| BrokerError::Internal("ws broker: connection closed".into()))?;

            response_rx
                .await
                .map_err(|_| BrokerError::Internal("ws broker: connection closed".into()))
        }

        /// like `request`, but retried indefinitely (with the same backoff the connector uses)
        /// across reconnects, since the caller (a blocking receive) must never see a transient
        /// disconnect as a hard failure.
        async fn request_forever(&self, request: TcpRequest) -> Result<TcpResponse, BrokerError> {
            let mut backoff = Backoff::new();
            loop {
                // clone once per attempt: `TcpRequest` carries owned data anyway (profile/consumer),
                // and retries are rare (only on disconnect), so this isn't a hot path.
                match self.request(clone_request(&request)).await {
                    Ok(response) => return Ok(response),
                    Err(_) => tokio::time::sleep(backoff.next_delay()).await,
                }
            }
        }

        /// like `request`, but retried for up to `max_wait` before giving up. one-shot ops
        /// (`publish`/`ack`/`nack`/...) use this rather than a single bare attempt, so a call that
        /// lands right as the client is still completing its *initial* connect (or mid-reconnect
        /// after a transient drop) doesn't fail outright — it still surfaces a `BrokerError` if the
        /// connection genuinely doesn't come back within the window.
        async fn request_bounded(
            &self,
            request: TcpRequest,
            max_wait: Duration,
        ) -> Result<TcpResponse, BrokerError> {
            let deadline = tokio::time::Instant::now() + max_wait;
            let mut backoff = Backoff::new();
            loop {
                match self.request(clone_request(&request)).await {
                    Ok(response) => return Ok(response),
                    Err(err) => {
                        if tokio::time::Instant::now() >= deadline {
                            return Err(err);
                        }
                        tokio::time::sleep(backoff.next_delay()).await;
                    }
                }
            }
        }
    }

    /// how long a one-shot op (`publish`/`ack`/`nack`/...) retries before giving up — long enough to
    /// ride out the client's initial connect or a brief reconnect, short enough that a genuinely dead
    /// connection still surfaces an error to the caller promptly.
    const ONE_SHOT_RETRY_WINDOW: Duration = Duration::from_secs(3);

    struct PendingCleanup {
        pending: PendingMap,
        request_id: Uuid,
    }

    impl Drop for PendingCleanup {
        fn drop(&mut self) {
            self.pending.lock().remove(&self.request_id);
        }
    }

    fn clone_request(request: &TcpRequest) -> TcpRequest {
        // `TcpRequest` isn't `Clone` (mirrors `BrokerMessage` etc., which aren't either); round-trip
        // through JSON rather than adding a derive that would ripple into every payload type it wraps.
        serde_json::from_str(&serde_json::to_string(request).expect("TcpRequest always serializes"))
            .expect("TcpRequest round-trips through its own wire format")
    }

    async fn run_supervisor(
        url: String,
        api_key: Option<String>,
        connection: watch::Sender<Option<ConnectionHandle>>,
    ) {
        let mut backoff = Backoff::new();
        loop {
            match connect_once(&url, api_key.as_deref()).await {
                Ok((write_tx, pending, reader)) => {
                    backoff.reset();
                    let _ = connection.send(Some(ConnectionHandle { write_tx, pending }));
                    // blocks until the connection drops (reader task ends).
                    let _ = reader.await;
                    let _ = connection.send(None);
                }
                Err(err) => {
                    warn!("ws broker: connect to {url} failed: {err}");
                }
            }
            tokio::time::sleep(backoff.next_delay()).await;
        }
    }

    /// establish one connection: upgrade, split into read/write halves, spawn the writer (draining
    /// an mpsc so callers never touch the socket directly) and the reader (dispatching each incoming
    /// frame to its awaiting caller via the pending map). returns once connected; the returned join
    /// handle resolves when the reader task ends (i.e. the connection has dropped).
    async fn connect_once(
        url: &str,
        api_key: Option<&str>,
    ) -> Result<
        (
            mpsc::UnboundedSender<Message>,
            PendingMap,
            tokio::task::JoinHandle<()>,
        ),
        BrokerError,
    > {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let mut request = url
            .into_client_request()
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        if let Some(key) = api_key.filter(|key| !key.is_empty()) {
            let value = format!("Bearer {key}")
                .parse()
                .map_err(|_| BrokerError::Internal("ws broker: invalid api key".into()))?;
            request.headers_mut().insert("Authorization", value);
        }

        let (stream, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|err| BrokerError::Internal(format!("ws broker connect: {err}")))?;
        let (mut sink, mut source) = stream.split();

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Message>();

        tokio::spawn(async move {
            while let Some(message) = write_rx.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
        });

        let reader_pending = pending.clone();
        let reader = tokio::spawn(async move {
            while let Some(next) = source.next().await {
                let Ok(Message::Text(text)) = next else {
                    break;
                };
                let Ok(frame) = serde_json::from_str::<WsResponseFrame>(&text) else {
                    continue;
                };
                if let Some(sender) = reader_pending.lock().remove(&frame.request_id) {
                    let _ = sender.send(frame.body);
                }
            }
            // connection ended (error or close): drop every still-pending sender, which turns each
            // awaiting `request()` call's oneshot receive into a retryable error.
            reader_pending.lock().clear();
        });

        Ok((write_tx, pending, reader))
    }

    #[async_trait]
    impl Broker for WsBroker {
        fn supports_workflow_result_channels(&self) -> bool {
            // like the other pass-through transports (Tcp/Http), capability depends on whatever
            // backend runinator-ws holds, not on this transport — and a worker's only actual need
            // (publish_result) is always in the relay's policy allow-list.
            true
        }

        async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
            match self
                .request_bounded(TcpRequest::Publish { message }, ONE_SHOT_RETRY_WINDOW)
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::Receive {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::Delivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_for(
            &self,
            profile: &ConsumerProfile,
        ) -> Result<BrokerDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveFor {
                    profile: profile.clone(),
                })
                .await?
            {
                TcpResponse::Delivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn ack(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::Ack {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn nack(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::Nack {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::PublishControl { command },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveControl {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::ControlDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_control_for(
            &self,
            profile: &ConsumerProfile,
        ) -> Result<ControlDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveControlFor {
                    profile: profile.clone(),
                })
                .await?
            {
                TcpResponse::ControlDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn ack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::AckControl {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn nack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::NackControl {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError> {
            match self
                .request_bounded(TcpRequest::PublishResult { message }, ONE_SHOT_RETRY_WINDOW)
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveResult {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::ResultDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn ack_result(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::AckResult {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn nack_result(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::NackResult {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
            match self
                .request_bounded(TcpRequest::PublishWake { message }, ONE_SHOT_RETRY_WINDOW)
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveWake {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::WakeDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn ack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::AckWake {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn nack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::NackWake {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::PublishIngress { message },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveIngress {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::IngressDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn ack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::AckIngress {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn nack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::NackIngress {
                        consumer: consumer.to_string(),
                        delivery_id,
                    },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
            match self
                .request_bounded(TcpRequest::PublishEvent { message }, ONE_SHOT_RETRY_WINDOW)
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveEvent {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::EventDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }
    }

    fn unexpected_response() -> BrokerError {
        BrokerError::Internal("ws broker: unexpected response variant for this request".into())
    }
}

#[cfg(feature = "ws")]
pub use imp::WsBroker;

#[cfg(not(feature = "ws"))]
pub struct WsBroker;

#[cfg(not(feature = "ws"))]
impl WsBroker {
    pub fn connect(_url: String, _api_key: Option<String>) -> Self {
        Self
    }
}

#[cfg(not(feature = "ws"))]
fn ws_feature_error() -> BrokerError {
    BrokerError::FeatureDisabled("ws")
}

#[async_trait]
#[cfg(not(feature = "ws"))]
impl Broker for WsBroker {
    async fn publish(&self, _message: BrokerMessage) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        Err(ws_feature_error())
    }

    async fn ack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn nack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn publish_control(&self, _command: ControlCommand) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        Err(ws_feature_error())
    }

    async fn ack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn nack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn publish_result(&self, _message: ResultMessage) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn receive_result(&self, _consumer: &str) -> Result<ResultDelivery, BrokerError> {
        Err(ws_feature_error())
    }

    async fn ack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn nack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn publish_wake(&self, _message: WakeMessage) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn receive_wake(&self, _consumer: &str) -> Result<WakeDelivery, BrokerError> {
        Err(ws_feature_error())
    }

    async fn ack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn nack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn publish_ingress(&self, _message: IngressMessage) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn receive_ingress(&self, _consumer: &str) -> Result<IngressDelivery, BrokerError> {
        Err(ws_feature_error())
    }

    async fn ack_ingress(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn nack_ingress(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn publish_event(&self, _message: EventMessage) -> Result<(), BrokerError> {
        Err(ws_feature_error())
    }

    async fn receive_event(&self, _consumer: &str) -> Result<EventDelivery, BrokerError> {
        Err(ws_feature_error())
    }
}
