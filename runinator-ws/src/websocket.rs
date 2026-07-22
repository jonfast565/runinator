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
    tcp::types::TcpRequest,
    ws::types::{WsRequestFrame, WsResponseFrame},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::authz;
use crate::events::{AppEventKind, EventSender};
use crate::models;
use crate::repository;

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
    send_json(tx, &models::WorkflowRunResponse { run, nodes }).await?;
    Ok(())
}

pub(crate) fn merge_json(
    target: &mut runinator_models::value::Value,
    overlay: runinator_models::value::Value,
) {
    use runinator_models::value::Value;
    match (target, overlay) {
        (Value::Object(target), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match target.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, overlay) => *target = overlay,
    }
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
                            if !authz::org_visible(&ctx, event.org_id) {
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
                            if !authz::org_visible(&ctx, event.org_id) {
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
                                && authz::org_visible(&ctx, event.org_id)
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
                            if !authz::org_visible(&ctx, event.org_id) {
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
        while let Some(msg) = rx_ws.next().await {
            let text = match msg {
                Ok(Message::Text(text)) => text,
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            };
            let Ok(frame) = serde_json::from_str::<WsRequestFrame>(&text) else {
                continue;
            };
            let db = db.clone();
            let broker = broker.clone();
            let ctx = ctx.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let response =
                    handle_desktop_worker_request(db.as_ref(), broker.as_ref(), &ctx, frame.body)
                        .await;
                let Ok(payload) =
                    serde_json::to_string(&WsResponseFrame::new(frame.request_id, response))
                else {
                    return;
                };
                let _ = tx.lock().await.send(Message::Text(payload.into())).await;
            });
        }
        log::info!("WebSocket connection closed for /ws/desktop-worker");
    })
}

/// the policy allow-list and replica-ownership check for the desktop-worker relay, ahead of the
/// generic dispatch every other transport uses. a desktop worker only ever legitimately needs
/// `receive_for`/`ack`/`nack` (action channel), `receive_control[_for]`/`ack_control`/`nack_control`
/// (control channel), and `publish_result` (result channel) — everything else (publishing
/// actions/control/wake/ingress, the fan-out events channel, and the untargeted general `receive`)
/// is refused outright.
async fn handle_desktop_worker_request<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    ctx: &AuthContext,
    request: TcpRequest,
) -> runinator_broker::tcp::types::TcpResponse {
    use runinator_broker::tcp::types::TcpResponse;

    match &request {
        TcpRequest::ReceiveFor { profile } => {
            if !profile.exclusive {
                return TcpResponse::Error {
                    message: "desktop-worker relay requires an exclusive consumer profile".into(),
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
        }
        TcpRequest::Ack { .. }
        | TcpRequest::Nack { .. }
        | TcpRequest::ReceiveControl { .. }
        | TcpRequest::AckControl { .. }
        | TcpRequest::NackControl { .. }
        | TcpRequest::PublishResult { .. } => {}
        _ => {
            return TcpResponse::Error {
                message: "operation not permitted over the desktop-worker relay".into(),
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
            message: "replica_id is not owned by the connecting identity".into(),
        }),
        Ok(None) => Some(TcpResponse::Error {
            message: "unknown replica_id".into(),
        }),
        Err(err) => Some(TcpResponse::Error {
            message: err.to_string(),
        }),
    }
}
