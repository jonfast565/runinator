use std::time::Duration;

use axum::{
    Extension,
    extract::{
        Path,
        ws::{Message, WebSocketUpgrade},
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use runinator_database::interfaces::DatabaseImpl;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::events::{AppEvent, EventSender};
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
    run_id: i64,
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
    node_run_id: i64,
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
    run_id: i64,
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

pub(crate) fn merge_json(target: &mut serde_json::Value, overlay: serde_json::Value) {
    match (target, overlay) {
        (serde_json::Value::Object(target), serde_json::Value::Object(overlay)) => {
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
    Path(run_id): Path<i64>,
    ws: WebSocketUpgrade,
) -> Response {
    log::info!("WebSocket upgrade request for /ws/workflow-runs/{}", run_id);
    ws.on_upgrade(move |socket| async move {
        log::info!("WebSocket connection established for /ws/workflow-runs/{}", run_id);
        let (mut tx, mut rx_ws) = socket.split();
        let _ = send_workflow_run(db.as_ref(), &mut tx, run_id).await;
        let mut event_rx = events.subscribe();
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Ok(event) => {
                            let relevant = matches!(&event,
                                AppEvent::WorkflowRunChanged { run_id: id } if *id == run_id
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
        log::info!("WebSocket connection closed for /ws/workflow-runs/{}", run_id);
    })
}

pub(crate) async fn ws_workflow_node_run_stream<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(node_run_id): Path<i64>,
    ws: WebSocketUpgrade,
) -> Response {
    log::info!("WebSocket upgrade request for /ws/workflow-node-runs/{}/stream", node_run_id);
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
                            if matches!(&event, AppEvent::WorkflowRunChanged { .. }) {
                                if send_workflow_node_run_chunks(db.as_ref(), &mut tx, node_run_id, &mut cursor, 100).await.is_err() {
                                    break;
                                }
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
    Path(run_id): Path<i64>,
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
                            let is_chunk = matches!(&event, AppEvent::RunChunkAdded { run_id: id } if *id == run_id);
                            let is_done = matches!(&event, AppEvent::RunStatusChanged { run_id: id, terminal: true } if *id == run_id);
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
