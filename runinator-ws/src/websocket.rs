use std::time::Duration;
use uuid::Uuid;

use axum::{
    Extension,
    extract::{
        Path,
        ws::{Message, WebSocketUpgrade},
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use runinator_broker::{
    Broker,
    dispatch::dispatch,
    tcp::types::{TcpRequest, TcpResponse},
    ws::types::{WsRequestFrame, WsResponseFrame},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::events::{AppEventKind, EventSender};
use crate::models;
use crate::openapi::docs::{EndpointDoc, Example, endpoint};
use crate::repository;
use runinator_ws_middleware::authz::AuthContextExt;

pub(crate) async fn send_json<T: Serialize>(
    tx: &mut futures::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    value: &T,
) -> Result<(), ()> {
    let payload = serde_json::to_string(value).map_err(|_| ())?;
    tx.send(Message::Text(payload.into())).await.map_err(|_| ())
}

pub(crate) async fn send_run_chunks<T: DatabaseImpl>(
    db: &T,
    tx: &mut futures::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    run_id: Uuid,
    cursor: &mut Option<i64>,
    limit: i64,
) -> Result<(), ()> {
    let chunks = repository::fetch_run_chunks(db, run_id, *cursor, limit)
        .await
        .map_err(|_| ())?;
    for chunk in &chunks {
        send_json(tx, chunk).await?;
        *cursor = Some(chunk.sequence);
    }
    Ok(())
}

pub(crate) async fn send_workflow_node_run_chunks<T: DatabaseImpl>(
    db: &T,
    tx: &mut futures::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    node_run_id: Uuid,
    cursor: &mut Option<i64>,
    limit: i64,
) -> Result<(), ()> {
    let chunks = repository::fetch_workflow_node_run_chunks(db, node_run_id, *cursor, limit)
        .await
        .map_err(|_| ())?;
    for chunk in &chunks {
        send_json(tx, chunk).await?;
        *cursor = Some(chunk.sequence);
    }
    Ok(())
}

pub(crate) async fn send_workflow_run<T: DatabaseImpl>(
    db: &T,
    tx: &mut futures::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    run_id: Uuid,
) -> Result<(), ()> {
    let Some((run, nodes)) = repository::fetch_workflow_run(db, run_id)
        .await
        .map_err(|_| ())?
    else {
        return Err(());
    };
    send_json(tx, &models::WorkflowRunResponse::new(run, nodes)).await?;
    Ok(())
}

pub(crate) async fn ws_events(
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    ws: WebSocketUpgrade,
) -> Response {
    log::info!("WebSocket upgrade request for /ws/events");
    let mut rx = events.subscribe();
    ws.on_upgrade(move |socket| async move {
        log::info!("WebSocket connection established for /ws/events");
        let (mut tx, mut rx_ws) = socket.split();
        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Ok(event) => {
                            // org-scoped egress: drop cross-tenant hints; unscoped events stay visible.
                            if !ctx.org_visible(event.org_id) {
                                continue;
                            }
                            if send_json(&mut tx, &event).await.is_err() {
                                log::warn!("Failed to send event to WebSocket, closing connection");
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(missed)) => {
                            log::warn!("WebSocket client lagged, missed {} events", missed);
                            if send_json(
                                &mut tx,
                                &serde_json::json!({ "type": "resync", "missed": missed }),
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            log::info!("Event broadcast channel closed");
                            break;
                        }
                    }
                }
                msg = rx_ws.next() => {
                    match msg {
                        Some(Ok(Message::Close(frame))) => {
                            log::info!("WebSocket closed by client: {:?}", frame);
                            break;
                        }
                        Some(Err(e)) => {
                            log::error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            log::info!("WebSocket connection terminated by client");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        log::info!("WebSocket connection closed for /ws/events");
    })
}

pub(crate) async fn ws_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(run_id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Response {
    log::info!("WebSocket upgrade request for /ws/workflow-runs/{}", run_id);
    ws.on_upgrade(move |socket| async move {
        log::info!(
            "WebSocket connection established for /ws/workflow-runs/{}",
            run_id
        );
        let (mut tx, mut rx_ws) = socket.split();
        let _ = send_workflow_run(db.as_ref(), &mut tx, run_id).await;
        let mut event_rx = events.subscribe();
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Ok(event) => {
                            if !ctx.org_visible(event.org_id) {
                                continue;
                            }
                            let relevant = matches!(
                                &event.kind,
                                AppEventKind::WorkflowRunChanged { run_id: id } if *id == run_id
                            );
                            if !relevant {
                                continue;
                            }
                            let Ok(_) = send_workflow_run(db.as_ref(), &mut tx, run_id).await else {
                                break;
                            };
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if send_workflow_run(db.as_ref(), &mut tx, run_id).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                msg = rx_ws.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
            }
        }
        log::info!(
            "WebSocket connection closed for /ws/workflow-runs/{}",
            run_id
        );
    })
}

pub(crate) async fn ws_workflow_node_run_stream<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(node_run_id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Response {
    log::info!(
        "WebSocket upgrade request for /ws/workflow-node-runs/{}/stream",
        node_run_id
    );
    ws.on_upgrade(move |socket| async move {
        log::info!("WebSocket connection established for /ws/workflow-node-runs/{}/stream", node_run_id);
        let (mut tx, mut rx_ws) = socket.split();
        let mut cursor: Option<i64> = None;
        if send_workflow_node_run_chunks(db.as_ref(), &mut tx, node_run_id, &mut cursor, 500)
            .await
            .is_err()
        {
            return;
        }
        let mut event_rx = events.subscribe();
        let mut poll_interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Ok(event) => {
                            if matches!(&event.kind, AppEventKind::WorkflowRunChanged { .. })
                                && ctx.org_visible(event.org_id)
                                && send_workflow_node_run_chunks(db.as_ref(), &mut tx, node_run_id, &mut cursor, 100).await.is_err() {
                                    break;
                                }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if send_workflow_node_run_chunks(db.as_ref(), &mut tx, node_run_id, &mut cursor, 500).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = poll_interval.tick() => {
                    if send_workflow_node_run_chunks(db.as_ref(), &mut tx, node_run_id, &mut cursor, 100).await.is_err() {
                        break;
                    }
                }
                msg = rx_ws.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
            }
        }
        log::info!("WebSocket connection closed for /ws/workflow-node-runs/{}/stream", node_run_id);
    })
}

pub(crate) async fn ws_run_stream<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(run_id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Response {
    log::info!("WebSocket upgrade request for /ws/run-stream/{}", run_id);
    ws.on_upgrade(move |socket| async move {
        log::info!("WebSocket connection established for /ws/run-stream/{}", run_id);
        let (mut tx, mut rx_ws) = socket.split();
        let mut cursor: Option<i64> = None;
        if send_run_chunks(db.as_ref(), &mut tx, run_id, &mut cursor, 500)
            .await
            .is_err()
        {
            return;
        }
        let mut event_rx = events.subscribe();
        let mut poll_interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Ok(event) => {
                            if !ctx.org_visible(event.org_id) {
                                continue;
                            }
                            let is_chunk = matches!(
                                &event.kind,
                                AppEventKind::RunChunkAdded { run_id: id } if *id == run_id
                            );
                            let is_done = matches!(
                                &event.kind,
                                AppEventKind::RunStatusChanged { run_id: id, terminal: true } if *id == run_id
                            );
                            if is_chunk || is_done {
                                if send_run_chunks(db.as_ref(), &mut tx, run_id, &mut cursor, 100).await.is_err() {
                                    break;
                                }
                                if is_done {
                                    break;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if send_run_chunks(db.as_ref(), &mut tx, run_id, &mut cursor, 500).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = poll_interval.tick() => {
                    if send_run_chunks(db.as_ref(), &mut tx, run_id, &mut cursor, 100).await.is_err() {
                        break;
                    }
                }
                msg = rx_ws.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
            }
        }
        log::info!("WebSocket connection closed for /ws/run-stream/{}", run_id);
    })
}

/// relays broker traffic for an external, lower-trust worker (e.g. `runinator-desktop-agent`) that
/// can't reach the internal broker (RabbitMQ) directly, but can reach this already-authenticated,
/// already-exposed endpoint. dispatches against the exact same `Arc<dyn Broker>` every other part of
/// this service uses, so it's correct regardless of the deployment's backend, and it inherits the
/// standard auth middleware (already applied to every `/ws/*` route) for free.
///
/// unlike `ws_events` (fan-out, no ack, read-only), this is bidirectional and multiplexed: each
/// incoming request is dispatched on its own spawned task so a slow `receive_for`/`receive_control`
/// never blocks a concurrent `ack` arriving moments later on the same connection.
pub(crate) async fn ws_desktop_worker<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(broker): Extension<Arc<dyn Broker>>,
    Extension(ctx): Extension<AuthContext>,
    ws: WebSocketUpgrade,
) -> Response {
    log::info!("WebSocket upgrade request for /ws/desktop-worker");
    ws.on_upgrade(move |socket| async move {
        log::info!("WebSocket connection established for /ws/desktop-worker");
        let (tx, mut rx_ws) = socket.split();
        let tx = Arc::new(tokio::sync::Mutex::new(tx));
        let in_flight = Arc::new(tokio::sync::Semaphore::new(RELAY_MAX_IN_FLIGHT));

        // server-side keepalive. the client pings too, but only this side can prove *its* writes
        // still reach the agent — and an agent that vanished without a close frame otherwise leaves
        // this connection, and every broker consumer parked behind it, alive indefinitely.
        let ping_tx = tx.clone();
        let ping = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(RELAY_PING_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await; // the first tick fires immediately; skip it.
            loop {
                ticker.tick().await;
                if ping_tx
                    .lock()
                    .await
                    .send(Message::Ping(Default::default()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        loop {
            // bounded read, mirroring the client: a half-open connection (an agent whose route
            // disappeared) never produces a close frame or an error, so waiting on `next()` alone
            // holds this task and its broker consumers open forever.
            let msg = match tokio::time::timeout(RELAY_IDLE_TIMEOUT, rx_ws.next()).await {
                Ok(Some(msg)) => msg,
                Ok(None) => break,
                Err(_) => {
                    log::warn!(
                        "/ws/desktop-worker idle for {}s with no frame; closing",
                        RELAY_IDLE_TIMEOUT.as_secs()
                    );
                    break;
                }
            };
            let text = match msg {
                Ok(Message::Text(text)) => text,
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            };
            let Ok(frame) = serde_json::from_str::<WsRequestFrame>(&text) else {
                continue;
            };

            // `try_acquire`, never an awaited acquire: parked `receive_for` calls hold their permit
            // for as long as they wait, so blocking the read loop on a permit would stop us reading
            // the very `ack` frames that would free one. refusing is safe — every client op retries.
            let Ok(permit) = Arc::clone(&in_flight).try_acquire_owned() else {
                let response = TcpResponse::Error {
                    message: crate::errors::RELAY_BUSY
                        .error(format!(
                            "more than {RELAY_MAX_IN_FLIGHT} requests in flight"
                        ))
                        .to_string(),
                };
                let Ok(payload) =
                    serde_json::to_string(&WsResponseFrame::new(frame.request_id, response))
                else {
                    continue;
                };
                if tx
                    .lock()
                    .await
                    .send(Message::Text(payload.into()))
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            };

            let db = db.clone();
            let broker = broker.clone();
            let ctx = ctx.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let _permit = permit;
                // held so a delivery can be handed back if the reply never lands.
                let stranded = StrandedDelivery::consumer_for(&frame.body);
                let response =
                    handle_desktop_worker_request(db.as_ref(), broker.as_ref(), &ctx, frame.body)
                        .await;
                let stranded = stranded.zip_response(&response);
                let Ok(payload) =
                    serde_json::to_string(&WsResponseFrame::new(frame.request_id, response))
                else {
                    return;
                };
                if tx
                    .lock()
                    .await
                    .send(Message::Text(payload.into()))
                    .await
                    .is_err()
                {
                    // the socket died between the broker handing us a delivery and us forwarding it.
                    // the agent never saw it, so nobody will ever ack it — hand it straight back
                    // rather than leaving it leased to a consumer that no longer exists.
                    if let Some(stranded) = stranded {
                        stranded.nack(broker.as_ref()).await;
                    }
                }
            });
        }
        ping.abort();
        log::info!("WebSocket connection closed for /ws/desktop-worker");
    })
}

/// how many relay requests one connection may have in flight at once. a legitimate agent holds at
/// most a couple (one parked `receive_for`, one parked `receive_control_for`) plus short-lived
/// acks/publishes, so this is far above normal use and only bites a misbehaving or looping client.
const RELAY_MAX_IN_FLIGHT: usize = 64;

/// how often this side pings an otherwise-idle relay connection.
const RELAY_PING_INTERVAL: Duration = Duration::from_secs(20);

/// how long this side tolerates a relay connection with no inbound frame before closing it. the
/// client pings every 20s and we answer every ping, so a live agent refreshes this comfortably.
const RELAY_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// a delivery this connection took off the broker but may not manage to forward.
///
/// the relay is the only consumer boundary where "received" and "delivered to the worker" are
/// different events: `dispatch` takes a delivery from the broker, and the socket can die before the
/// reply carrying it reaches the agent. without this, that delivery stays leased to a consumer that
/// will never ack it and the work stalls until the lease expires.
enum StrandedDelivery {
    Action { consumer: String, delivery_id: Uuid },
    Control { consumer: String, delivery_id: Uuid },
    Agent { consumer: String, delivery_id: Uuid },
}

/// which consumer a receive-shaped request would be taking a delivery for, if any.
enum StrandedConsumer {
    Action(String),
    Control(String),
    Agent(String),
    None,
}

impl StrandedDelivery {
    fn consumer_for(request: &TcpRequest) -> StrandedConsumer {
        match request {
            TcpRequest::ReceiveFor { profile } => StrandedConsumer::Action(profile.id.clone()),
            TcpRequest::ReceiveControlFor { profile } => {
                StrandedConsumer::Control(profile.id.clone())
            }
            TcpRequest::ReceiveControl { consumer } => StrandedConsumer::Control(consumer.clone()),
            TcpRequest::ReceiveAgentFor { profile } => StrandedConsumer::Agent(profile.id.clone()),
            TcpRequest::ReceiveAgent { consumer } => StrandedConsumer::Agent(consumer.clone()),
            _ => StrandedConsumer::None,
        }
    }

    async fn nack(self, broker: &dyn Broker) {
        let (channel, result) = match self {
            Self::Action {
                consumer,
                delivery_id,
            } => ("action", broker.nack(&consumer, delivery_id).await),
            Self::Control {
                consumer,
                delivery_id,
            } => ("control", broker.nack_control(&consumer, delivery_id).await),
            Self::Agent {
                consumer,
                delivery_id,
            } => ("agent", broker.nack_agent(&consumer, delivery_id).await),
        };
        match result {
            Ok(()) => log::warn!(
                "/ws/desktop-worker: returned an undelivered {channel} delivery after the connection dropped"
            ),
            Err(err) => log::warn!(
                "/ws/desktop-worker: failed to return an undelivered {channel} delivery: {err}"
            ),
        }
    }
}

impl StrandedConsumer {
    /// pair the consumer with the delivery the broker actually produced, if it produced one.
    fn zip_response(self, response: &TcpResponse) -> Option<StrandedDelivery> {
        match (self, response) {
            (Self::Action(consumer), TcpResponse::Delivery { delivery }) => {
                Some(StrandedDelivery::Action {
                    consumer,
                    delivery_id: delivery.delivery_id,
                })
            }
            (Self::Control(consumer), TcpResponse::ControlDelivery { delivery }) => {
                Some(StrandedDelivery::Control {
                    consumer,
                    delivery_id: delivery.delivery_id,
                })
            }
            (Self::Agent(consumer), TcpResponse::AgentDelivery { delivery }) => {
                Some(StrandedDelivery::Agent {
                    consumer,
                    delivery_id: delivery.delivery_id,
                })
            }
            _ => None,
        }
    }
}

/// the policy allow-list and replica-ownership check for the desktop-worker relay, ahead of the
/// generic dispatch every other transport uses. a desktop worker only ever legitimately needs
/// `receive_for`/`ack`/`nack` (action channel), replica-targeted control, replica-targeted agent
/// directives, `publish_result`, and payload-gated directive results on ingress.
async fn handle_desktop_worker_request<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    ctx: &AuthContext,
    mut request: TcpRequest,
) -> runinator_broker::tcp::types::TcpResponse {
    use runinator_broker::tcp::types::TcpResponse;

    match &mut request {
        TcpRequest::ReceiveFor { profile } => {
            if !profile.exclusive {
                return TcpResponse::Error {
                    message: crate::errors::RELAY_NOT_EXCLUSIVE.bare().to_string(),
                };
            }
            if let Some(response) = refuse_unowned_replica(db, ctx, profile).await {
                return response;
            }
        }
        // control consumption is deliberately non-exclusive (a run-wide `Any` control must still
        // reach the desktop), so only the replica-ownership check applies here.
        TcpRequest::ReceiveControlFor { profile } => {
            if let Some(response) = refuse_unowned_replica(db, ctx, profile).await {
                return response;
            }
            // a relay consumer must never win a run-wide Any control from the shared queue. making
            // this receive profile exclusive preserves Replica matches while excluding Any.
            profile.exclusive = true;
        }
        TcpRequest::ReceiveAgentFor { profile } => {
            if let Some(response) = refuse_unowned_replica(db, ctx, profile).await {
                return response;
            }
        }
        TcpRequest::Ack { .. }
        | TcpRequest::Nack { .. }
        | TcpRequest::AckControl { .. }
        | TcpRequest::NackControl { .. }
        | TcpRequest::AckAgent { .. }
        | TcpRequest::NackAgent { .. }
        | TcpRequest::PublishResult { .. } => {}
        TcpRequest::PublishIngress { message }
            if matches!(
                message.command,
                runinator_comm::WsIngressCommand::AgentDirectiveResult { .. }
            ) => {}
        _ => {
            return TcpResponse::Error {
                message: crate::errors::RELAY_OPERATION_REFUSED
                    .error(request.operation_name())
                    .to_string(),
            };
        }
    }
    dispatch(broker, request).await
}

/// refuse a profile whose replica_id exists but is not registered by the connecting identity, so a
/// desktop connection cannot impersonate another replica to receive its targeted deliveries.
async fn refuse_unowned_replica<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    profile: &runinator_comm::ConsumerProfile,
) -> Option<runinator_broker::tcp::types::TcpResponse> {
    use runinator_broker::tcp::types::TcpResponse;

    let replica_id = profile.replica_id?;
    match repository::fetch_replica(db, replica_id).await {
        Ok(Some(replica)) if replica.registered_by_principal_id == ctx.principal_id => None,
        Ok(Some(_)) => Some(TcpResponse::Error {
            message: crate::errors::RELAY_REPLICA_NOT_OWNED
                .error(replica_id)
                .to_string(),
        }),
        Ok(None) => Some(TcpResponse::Error {
            message: crate::errors::RELAY_UNKNOWN_REPLICA
                .error(replica_id)
                .to_string(),
        }),
        Err(err) => Some(TcpResponse::Error {
            message: crate::errors::RELAY_REPLICA_LOOKUP.error(err).to_string(),
        }),
    }
}

/// the websocket upgrade endpoints.
pub(crate) fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::get;
    axum::Router::new()
        .route("/ws/events", get(ws_events))
        .route(
            "/ws/workflow-runs/{id}",
            get(ws_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/ws/run-stream/{id}",
            get(ws_run_stream::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/ws/workflow-node-runs/{id}/stream",
            get(ws_workflow_node_run_stream::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/ws/desktop-worker",
            get(ws_desktop_worker::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub(crate) const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/ws/events",
        "WebSockets",
        "Subscribe to UI events",
        "Upgrades to a websocket stream of fan-out UI events emitted by this web-service replica.",
        false,
        None,
        &[],
        101,
        "websocket upgrade accepted",
        Example::None,
    ),
    endpoint(
        "get",
        "/ws/workflow-runs/{id}",
        "WebSockets",
        "Subscribe to one workflow run",
        "Upgrades to a websocket stream for workflow-run changes and node activity for one run.",
        false,
        None,
        &[],
        101,
        "websocket upgrade accepted",
        Example::None,
    ),
    endpoint(
        "get",
        "/ws/run-stream/{id}",
        "WebSockets",
        "Subscribe to task run output",
        "Upgrades to a websocket stream for chunks emitted by one low-level task run.",
        false,
        None,
        &[],
        101,
        "websocket upgrade accepted",
        Example::None,
    ),
    endpoint(
        "get",
        "/ws/workflow-node-runs/{id}/stream",
        "WebSockets",
        "Subscribe to node-run output",
        "Upgrades to a websocket stream for chunks emitted by one workflow node run.",
        false,
        None,
        &[],
        101,
        "websocket upgrade accepted",
        Example::None,
    ),
];
