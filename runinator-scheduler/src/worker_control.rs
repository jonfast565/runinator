#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use log::{error, info, warn};
use prost::Message;
use runinator_comm::{
    ControlKind,
    worker_control::{
        SchedulerControlAck, WorkerControlActionKind, WorkerControlEvent, WorkerControlEventKind,
    },
};
use runinator_models::errors::{RuntimeError, SendableError};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::JoinHandle,
};

use crate::{api::SchedulerApi, config::Config};

const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[async_trait]
pub trait WorkerControlApi: Send + Sync + 'static {
    async fn pause_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError>;
    async fn resume_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError>;
    async fn cancel_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError>;
}

#[async_trait]
impl WorkerControlApi for SchedulerApi {
    async fn pause_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        SchedulerApi::pause_workflow_run(self, workflow_run_id).await
    }

    async fn resume_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        SchedulerApi::resume_workflow_run(self, workflow_run_id).await
    }

    async fn cancel_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        SchedulerApi::cancel_workflow_run(self, workflow_run_id).await
    }
}

#[derive(Clone)]
struct AppState {
    api: Arc<dyn WorkerControlApi>,
}

pub async fn spawn_listener(
    config: &Config,
    api: Arc<dyn WorkerControlApi>,
    shutdown: Arc<Notify>,
) -> Result<Option<JoinHandle<Result<(), SendableError>>>, SendableError> {
    match config.worker_control_transport.as_str() {
        "disabled" => Ok(None),
        "http" => {
            let listener = bind_listener(config).await?;
            let addr = listener.local_addr()?;
            info!("Scheduler worker-control HTTP listener bound to {addr}");
            Ok(Some(tokio::spawn(async move {
                serve_http(listener, api, shutdown).await
            })))
        }
        "tcp" => {
            let listener = bind_listener(config).await?;
            let addr = listener.local_addr()?;
            info!("Scheduler worker-control TCP listener bound to {addr}");
            Ok(Some(tokio::spawn(async move {
                serve_tcp(listener, api, shutdown).await
            })))
        }
        other => Err(Box::new(RuntimeError::new(
            "scheduler.worker_control.unknown_transport".into(),
            format!("Unknown worker control transport '{other}'"),
        ))),
    }
}

async fn bind_listener(config: &Config) -> Result<TcpListener, SendableError> {
    let addr = format!(
        "{}:{}",
        config.worker_control_bind, config.worker_control_port
    );
    TcpListener::bind(addr).await.map_err(|err| {
        Box::new(RuntimeError::new(
            "scheduler.worker_control.bind".into(),
            err.to_string(),
        )) as SendableError
    })
}

pub async fn serve_http(
    listener: TcpListener,
    api: Arc<dyn WorkerControlApi>,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    let app = Router::new()
        .route("/worker-control/events", post(receive_http_event))
        .with_state(AppState { api });

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.notified().await;
        })
        .await
        .map_err(|err| {
            Box::new(RuntimeError::new(
                "scheduler.worker_control.http".into(),
                err.to_string(),
            )) as SendableError
        })
}

async fn receive_http_event(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let ack = match WorkerControlEvent::decode(body.as_ref()) {
        Ok(event) => handle_event(state.api.as_ref(), event).await,
        Err(err) => SchedulerControlAck::rejected(format!("Invalid protobuf payload: {err}")),
    };
    let status = if ack.accepted {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    protobuf_response(status, ack)
}

fn protobuf_response(status: StatusCode, ack: SchedulerControlAck) -> impl IntoResponse {
    let mut body = Vec::new();
    let response_status = match ack.encode(&mut body) {
        Ok(_) => status,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        response_status,
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        body,
    )
}

pub async fn serve_tcp(
    listener: TcpListener,
    api: Arc<dyn WorkerControlApi>,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    loop {
        tokio::select! {
            _ = shutdown.notified() => {
                info!("Scheduler worker-control TCP listener shutting down");
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, peer) = accepted.map_err(|err| {
                    Box::new(RuntimeError::new(
                        "scheduler.worker_control.tcp_accept".into(),
                        err.to_string(),
                    )) as SendableError
                })?;
                let api = Arc::clone(&api);
                tokio::spawn(async move {
                    if let Err(err) = handle_tcp_connection(stream, api).await {
                        warn!("Worker-control TCP connection from {peer} failed: {err}");
                    }
                });
            }
        }
    }
}

async fn handle_tcp_connection(
    mut stream: TcpStream,
    api: Arc<dyn WorkerControlApi>,
) -> Result<(), SendableError> {
    let frame = match read_frame(&mut stream).await {
        Ok(frame) => frame,
        Err(err) => {
            write_ack(&mut stream, SchedulerControlAck::rejected(err.to_string())).await?;
            return Ok(());
        }
    };
    let ack = match WorkerControlEvent::decode(frame.as_slice()) {
        Ok(event) => handle_event(api.as_ref(), event).await,
        Err(err) => SchedulerControlAck::rejected(format!("Invalid protobuf payload: {err}")),
    };
    write_ack(&mut stream, ack).await
}

async fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, SendableError> {
    let len = stream
        .read_u32()
        .await
        .map_err(io_error("read_frame_len"))? as usize;
    if len > MAX_FRAME_BYTES {
        return Err(Box::new(RuntimeError::new(
            "scheduler.worker_control.frame_too_large".into(),
            format!("Frame length {len} exceeds maximum {MAX_FRAME_BYTES}"),
        )));
    }

    let mut frame = vec![0; len];
    stream
        .read_exact(&mut frame)
        .await
        .map_err(io_error("read_frame"))?;
    Ok(frame)
}

async fn write_ack(stream: &mut TcpStream, ack: SchedulerControlAck) -> Result<(), SendableError> {
    let mut body = Vec::new();
    ack.encode(&mut body).map_err(|err| {
        Box::new(RuntimeError::new(
            "scheduler.worker_control.encode_ack".into(),
            err.to_string(),
        )) as SendableError
    })?;
    stream
        .write_u32(body.len() as u32)
        .await
        .map_err(io_error("write_ack_len"))?;
    stream
        .write_all(&body)
        .await
        .map_err(io_error("write_ack"))?;
    Ok(())
}

pub async fn handle_event(
    api: &dyn WorkerControlApi,
    event: WorkerControlEvent,
) -> SchedulerControlAck {
    let event_kind = match WorkerControlEventKind::try_from(event.kind) {
        Ok(kind) => kind,
        Err(_) => return SchedulerControlAck::rejected("Unknown worker control event kind"),
    };

    if event_kind != WorkerControlEventKind::ControlRequested {
        info!(
            "Worker-control event {:?} received from worker {} for workflow run {:?}",
            event_kind, event.worker_id, event.workflow_run_id
        );
        return SchedulerControlAck::accepted("Event recorded");
    }

    let Some(workflow_run_id) = event.workflow_run_id else {
        return SchedulerControlAck::rejected("Control request missing workflow_run_id");
    };
    let Some(raw_control_kind) = event.control_kind else {
        return SchedulerControlAck::rejected("Control request missing control_kind");
    };
    let control_kind = match WorkerControlActionKind::try_from(raw_control_kind)
        .ok()
        .and_then(|kind| ControlKind::try_from(kind).ok())
    {
        Some(kind) => kind,
        None => return SchedulerControlAck::rejected("Unknown control kind"),
    };

    let result = match control_kind {
        ControlKind::Cancel => api.cancel_workflow_run(workflow_run_id).await,
        ControlKind::Pause => api.pause_workflow_run(workflow_run_id).await,
        ControlKind::Resume => api.resume_workflow_run(workflow_run_id).await,
    };

    match result {
        Ok(()) => SchedulerControlAck::accepted("Control request applied"),
        Err(err) => {
            error!(
                "Failed to apply {:?} for workflow run {} from worker {}: {}",
                control_kind, workflow_run_id, event.worker_id, err
            );
            SchedulerControlAck::rejected(err.to_string())
        }
    }
}

fn io_error(context: &'static str) -> impl FnOnce(std::io::Error) -> SendableError {
    move |err| {
        Box::new(RuntimeError::new(
            format!("scheduler.worker_control.{context}"),
            err.to_string(),
        )) as SendableError
    }
}
