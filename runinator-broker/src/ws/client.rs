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
//! `receive_for`/`receive_control` retry indefinitely across transient reconnects (per
//! [`crate::Broker::receive_for`]'s "wait for and retrieve the next delivery" contract), while a
//! rejected credential is returned immediately as [`BrokerError::Unauthorized`]. One-shot ops
//! (`publish`, `ack`, `nack`, ...) retry for a few seconds (long enough to ride out the client's
//! initial connect or a brief reconnect) but likewise return a rejected credential immediately.

use async_trait::async_trait;
use uuid::Uuid;

// only the live client reports a connection state; the stub below has none to report.
#[cfg(feature = "ws")]
use crate::{
    AgentCommand, AgentDelivery, ConnectionState, ConsumerProfile, EffectDelivery, EffectMessage,
    EffectResultDelivery, EffectResultMessage,
};
use crate::{
    Broker, BrokerError, ControlCommand, ControlDelivery, EventDelivery, EventMessage,
    IngressDelivery, IngressMessage, WakeDelivery, WakeMessage,
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
        write_tx: mpsc::Sender<Message>,
        pending: PendingMap,
    }

    pub struct WsBroker {
        connection: watch::Receiver<Option<ConnectionHandle>>,
        state: watch::Receiver<ConnectionState>,
        // kept alive for the supervisor task's lifetime; dropping the broker drops this and ends it.
        _supervisor: tokio::task::JoinHandle<()>,
    }

    impl WsBroker {
        /// connect (in the background) to `url` (a `ws://`/`wss://` endpoint), presenting `api_key`
        /// as a bearer token on the upgrade request. returns immediately; the first request made
        /// before the initial connection completes simply waits, same as any later reconnect.
        pub fn connect(url: String, api_key: Option<String>) -> Self {
            let (tx, rx) = watch::channel(None);
            let (state_tx, state_rx) = watch::channel(ConnectionState::Connecting);
            let supervisor = tokio::spawn(run_supervisor(url, api_key, tx, state_tx));
            Self {
                connection: rx,
                state: state_rx,
                _supervisor: supervisor,
            }
        }

        /// watch this client's connection to the relay. see [`ConnectionState`].
        pub fn state(&self) -> watch::Receiver<ConnectionState> {
            self.state.clone()
        }

        /// one attempt: send `request` on whatever connection is currently live and await its reply.
        /// returns a retryable `Err` immediately if nothing is connected right now, rather than
        /// waiting — callers that must not give up (`receive_for`/`receive_control`) loop on this.
        async fn request(&self, request: TcpRequest) -> Result<TcpResponse, BrokerError> {
            let handle = self.connection.borrow().clone().ok_or_else(|| {
                match self.state.borrow().clone() {
                    ConnectionState::Unauthorized { reason } => BrokerError::Unauthorized(reason),
                    _ => BrokerError::Internal("ws broker: not connected".into()),
                }
            })?;

            let request_id = Uuid::new_v4();
            let (response_tx, response_rx) = oneshot::channel();
            handle.pending.lock().insert(request_id, response_tx);
            // raii: if this future is dropped before a reply arrives (e.g. a `tokio::select!` losing
            // a race), remove our own entry so it doesn't sit in the map forever.
            let _cleanup = PendingCleanup {
                pending: handle.pending.clone(),
                request_id,
            };

            let frame = WsRequestFrame::new(request_id, request);
            let payload = serde_json::to_string(&frame)
                .map_err(|err| BrokerError::Internal(err.to_string()))?;
            // `try_send`, never `send().await`: the queue is bounded now, and awaiting it would park
            // the caller indefinitely behind a writer that is itself blocked on a wedged socket —
            // exactly the case the read-idle deadline exists to break out of. a full queue is instead
            // a retryable error, which the retry loops above already know how to wait out.
            handle
                .write_tx
                .try_send(Message::Text(payload.into()))
                .map_err(|err| match err {
                    mpsc::error::TrySendError::Full(_) => {
                        BrokerError::Internal("ws broker: outbound queue full".into())
                    }
                    mpsc::error::TrySendError::Closed(_) => {
                        BrokerError::Internal("ws broker: connection closed".into())
                    }
                })?;

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
                    Err(err @ BrokerError::Unauthorized(_)) => return Err(err),
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
                    Err(err @ BrokerError::Unauthorized(_)) => return Err(err),
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
    ///
    /// sized for the worst case that actually matters: a failed `ack` on a *successfully executed*
    /// action makes the broker redeliver it and the worker run it a second time, with the external
    /// side effects already applied. that is far more expensive than waiting, so this is generous
    /// enough to cover a full reconnect (max backoff is 30s) on a high-latency link.
    const ONE_SHOT_RETRY_WINDOW: Duration = Duration::from_secs(30);

    /// how often the client emits a keepalive ping. the relay is mostly idle — a `receive_for`
    /// long-poll can sit quiet for minutes — so without traffic an idle-timeout intermediary (an
    /// ingress, an lb, a `kubectl port-forward`) will drop the connection. kept well under the
    /// usual 60s idle window so a ping always lands first.
    const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);

    /// how long the reader will sit with no inbound frame at all before declaring the connection
    /// dead and forcing a reconnect.
    ///
    /// this is the one that matters for an agent behind NAT. when a NAT table entry is evicted (or a
    /// route silently blackholes), the socket never errors: our keepalive pings vanish into the void,
    /// `receive_for` blocks forever, and the agent reports itself healthy while receiving no work at
    /// all — indefinitely. nothing else in the stack notices, because from the kernel's point of view
    /// there is still an established connection. three keepalive intervals means a healthy link
    /// always refreshes this well before it fires (every ping draws a pong, and both tungstenite and
    /// axum answer automatically), so only a genuinely one-way connection trips it.
    const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

    /// how many outbound frames may queue before callers are pushed back.
    ///
    /// the queue was previously unbounded, which on a slow uplink (a home connection publishing log
    /// chunks faster than it can drain) grows without limit until the process is killed. a full queue
    /// surfaces to callers as a retryable error, which the one-shot/forever retry loops already know
    /// how to wait out.
    const WRITE_QUEUE_DEPTH: usize = 1024;

    /// caps on a single inbound message and frame. tungstenite defaults to 64 MiB / 16 MiB, which is
    /// more than any relay reply legitimately needs and more than a memory-constrained agent should
    /// be willing to buffer on the say-so of the far end.
    const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
    const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

    /// bound on tungstenite's own write buffer, which defaults to unlimited. it only grows past the
    /// target size when writes to the socket are failing — precisely the wedged-uplink case — so
    /// leaving it unbounded reintroduces the growth [`WRITE_QUEUE_DEPTH`] exists to prevent, one
    /// layer further down.
    const MAX_WRITE_BUFFER_BYTES: usize = 8 * 1024 * 1024;

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
        // `TcpRequest` isn't `Clone` (mirrors `EffectMessage` etc., which aren't either); round-trip
        // through JSON rather than adding a derive that would ripple into every payload type it wraps.
        serde_json::from_str(&serde_json::to_string(request).expect("TcpRequest always serializes"))
            .expect("TcpRequest round-trips through its own wire format")
    }

    /// install a process-default rustls `CryptoProvider`, once, before the first `wss://` upgrade.
    ///
    /// every binary that builds a ws broker also links rustls with both `ring` (via jsonwebtoken) and
    /// `aws-lc-rs` (via the aws sdk), so rustls has no unambiguous default and the default-config path
    /// `connect_async` takes for a `wss://` url panics rather than erroring. `ring` matches what
    /// `runinator-ws` installs for the same reason. an `Err` means a provider is already installed —
    /// a binary that stated its own preference wins, which is why this never overrides.
    pub(super) fn install_crypto_provider() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    async fn run_supervisor(
        url: String,
        api_key: Option<String>,
        connection: watch::Sender<Option<ConnectionHandle>>,
        state: watch::Sender<ConnectionState>,
    ) {
        let mut backoff = Backoff::new();
        loop {
            let _ = state.send_if_modified(|current| {
                // a fatal state ends this task below; this guard also prevents any future terminal
                // state from briefly flapping back to connecting if another one is added.
                if current.is_fatal() {
                    return false;
                }
                *current = ConnectionState::Connecting;
                true
            });

            let reason = match connect_once(&url, api_key.as_deref()).await {
                Ok((write_tx, pending, reader)) => {
                    backoff.reset();
                    let _ = connection.send(Some(ConnectionHandle { write_tx, pending }));
                    let _ = state.send(ConnectionState::Connected);
                    // blocks until the connection drops (reader task ends).
                    let _ = reader.await;
                    let _ = connection.send(None);
                    "connection closed".to_string()
                }
                Err(BrokerError::Unauthorized(reason)) => {
                    // the credential is immutable for this broker instance, so retrying this upgrade
                    // can only hammer the relay with the same rejected key. publish the terminal
                    // state, then let requests return the typed error to the worker lifecycle.
                    let _ = state.send(ConnectionState::Unauthorized {
                        reason: reason.clone(),
                    });
                    log::error!(
                        "ws broker: {url} rejected our credential ({reason}); re-enrollment is required"
                    );
                    return;
                }
                Err(err) => {
                    warn!("ws broker: connect to {url} failed: {err}");
                    err.to_string()
                }
            };

            let delay = backoff.next_delay();
            let _ = state.send(ConnectionState::Reconnecting {
                retry_secs: delay.as_secs(),
                reason,
            });
            tokio::time::sleep(delay).await;
        }
    }

    /// classify a failed upgrade. a `401`/`403` means the relay understood us and refused the
    /// credential, which no amount of retrying will change; everything else is treated as transient.
    fn connect_error(err: tokio_tungstenite::tungstenite::Error) -> BrokerError {
        use tokio_tungstenite::tungstenite::Error as WsError;

        if let WsError::Http(response) = &err {
            let status = response.status();
            if status == 401 || status == 403 {
                return BrokerError::Unauthorized(format!("http {status}"));
            }
        }
        BrokerError::Internal(format!("ws broker connect: {err}"))
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
            mpsc::Sender<Message>,
            PendingMap,
            tokio::task::JoinHandle<()>,
        ),
        BrokerError,
    > {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

        let mut request = url
            .into_client_request()
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        if let Some(key) = api_key.filter(|key| !key.is_empty()) {
            let value = format!("Bearer {key}")
                .parse()
                .map_err(|_| BrokerError::Internal("ws broker: invalid api key".into()))?;
            request.headers_mut().insert("Authorization", value);
        }

        // field-by-field rather than a struct literal: `WebSocketConfig` is `#[non_exhaustive]`.
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(MAX_MESSAGE_BYTES);
        config.max_frame_size = Some(MAX_FRAME_BYTES);
        config.max_write_buffer_size = MAX_WRITE_BUFFER_BYTES;

        install_crypto_provider();
        // nagle off: relay traffic is small, latency-sensitive frames (an `ack` behind a parked
        // `receive_for`), not bulk data, so there is nothing to coalesce and 40ms of delay to lose.
        let (stream, _) = tokio_tungstenite::connect_async_with_config(request, Some(config), true)
            .await
            .map_err(connect_error)?;
        let (mut sink, mut source) = stream.split();

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (write_tx, mut write_rx) = mpsc::channel::<Message>(WRITE_QUEUE_DEPTH);

        // the writer owns the sink: it drains caller frames off `write_rx` and also emits a periodic
        // keepalive ping so an otherwise-idle relay isn't reaped. driving the keepalive from here ties
        // its lifetime to the writer, so it stops the instant the connection tears down (all `write_tx`
        // senders dropped, or the sink erroring) without a stray task to clean up.
        tokio::spawn(async move {
            let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
            keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            keepalive.tick().await; // the first tick fires immediately; skip it.
            loop {
                let message = tokio::select! {
                    outgoing = write_rx.recv() => match outgoing {
                        Some(message) => message,
                        None => break, // all senders dropped: the connection is being torn down.
                    },
                    _ = keepalive.tick() => Message::Ping(Default::default()),
                };
                if sink.send(message).await.is_err() {
                    break;
                }
            }
        });

        let reader_pending = pending.clone();
        // clone for the reader so it can answer inbound pings on the same connection.
        let pong_tx = write_tx.clone();
        let reader = tokio::spawn(async move {
            loop {
                // bounded read: see `READ_IDLE_TIMEOUT`. a silently-dropped route (an evicted NAT
                // entry) leaves the socket established forever with nothing arriving on it, so
                // waiting on `next()` alone is waiting for an event that will never come.
                let next = match tokio::time::timeout(READ_IDLE_TIMEOUT, source.next()).await {
                    Ok(Some(next)) => next,
                    Ok(None) => break, // stream ended.
                    Err(_) => {
                        warn!(
                            "ws broker: no frame in {}s (keepalive unanswered); reconnecting",
                            READ_IDLE_TIMEOUT.as_secs()
                        );
                        break;
                    }
                };
                let message = match next {
                    Ok(message) => message,
                    Err(_) => break, // transport error: the connection is gone.
                };
                match message {
                    Message::Text(text) => {
                        let Ok(frame) = serde_json::from_str::<WsResponseFrame>(&text) else {
                            continue;
                        };
                        if let Some(sender) = reader_pending.lock().remove(&frame.request_id) {
                            let _ = sender.send(frame.body);
                        }
                    }
                    // a keepalive ping is not a disconnect: answer it and keep the connection up.
                    // tungstenite also auto-queues a pong, but our writer only flushes on demand, so
                    // forward it explicitly to guarantee it leaves an otherwise-idle relay.
                    // `try_send` rather than awaiting: a full queue already means the connection is
                    // carrying plenty of traffic, so the peer has all the liveness evidence a pong
                    // would give it, and blocking the reader here would stall response dispatch.
                    Message::Ping(payload) => {
                        let _ = pong_tx.try_send(Message::Pong(payload));
                    }
                    // a pong (the answer to our own keepalive) or a stray binary frame is likewise not
                    // a disconnect; ignore it rather than tearing the connection down (the old code
                    // broke here, so a single keepalive frame forced a full reconnect).
                    Message::Pong(_) | Message::Binary(_) => {}
                    Message::Close(_) => break,
                    _ => {}
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
        fn supports_workflow_effect_channels(&self) -> bool {
            true
        }

        fn supports_agent_channel(&self) -> bool {
            true
        }

        fn connection_state(&self) -> Option<watch::Receiver<ConnectionState>> {
            Some(self.state())
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

        async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
            match self
                .request_bounded(TcpRequest::PublishAgent { command }, ONE_SHOT_RETRY_WINDOW)
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_agent(&self, consumer: &str) -> Result<AgentDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveAgent {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::AgentDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_agent_for(
            &self,
            profile: &ConsumerProfile,
        ) -> Result<AgentDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveAgentFor {
                    profile: profile.clone(),
                })
                .await?
            {
                TcpResponse::AgentDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn ack_agent(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::AckAgent {
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

        async fn nack_agent(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::NackAgent {
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

        async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
            match self
                .request_bounded(TcpRequest::PublishEffect { message }, ONE_SHOT_RETRY_WINDOW)
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_effect(&self, consumer: &str) -> Result<EffectDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveEffect {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::EffectDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_effect_for(
            &self,
            profile: &ConsumerProfile,
        ) -> Result<EffectDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveEffectFor {
                    profile: profile.clone(),
                })
                .await?
            {
                TcpResponse::EffectDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_infrastructure_effect(
            &self,
            consumer: &str,
        ) -> Result<EffectDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveInfrastructureEffect {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::EffectDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn ack_effect(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::AckEffect {
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

        async fn nack_effect(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::NackEffect {
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

        async fn publish_effect_result(
            &self,
            message: EffectResultMessage,
        ) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::PublishEffectResult { message },
                    ONE_SHOT_RETRY_WINDOW,
                )
                .await?
            {
                TcpResponse::Ok => Ok(()),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn receive_effect_result(
            &self,
            consumer: &str,
        ) -> Result<EffectResultDelivery, BrokerError> {
            match self
                .request_forever(TcpRequest::ReceiveEffectResult {
                    consumer: consumer.to_string(),
                })
                .await?
            {
                TcpResponse::EffectResultDelivery { delivery } => Ok(delivery),
                TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
                _ => Err(unexpected_response()),
            }
        }

        async fn ack_effect_result(
            &self,
            consumer: &str,
            delivery_id: Uuid,
        ) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::AckEffectResult {
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

        async fn nack_effect_result(
            &self,
            consumer: &str,
            delivery_id: Uuid,
        ) -> Result<(), BrokerError> {
            match self
                .request_bounded(
                    TcpRequest::NackEffectResult {
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

#[cfg(all(test, feature = "ws"))]
#[path = "client_tests.rs"]
mod tests;
