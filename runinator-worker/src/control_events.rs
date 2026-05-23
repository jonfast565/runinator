use chrono::Utc;
use prost::Message;
use reqwest::header::CONTENT_TYPE;
use runinator_comm::{
    ControlKind,
    worker_control::{SchedulerControlAck, WorkerControlEvent, WorkerControlEventKind},
};
use runinator_models::errors::{RuntimeError, SendableError};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use uuid::Uuid;

use crate::config::Config;

const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct SchedulerControlClient {
    worker_id: Uuid,
    transport: SchedulerControlTransport,
    http_client: reqwest::Client,
}

#[derive(Clone)]
enum SchedulerControlTransport {
    Disabled,
    Http(reqwest::Url),
    Tcp(String),
}

pub struct EventDetails {
    pub workflow_run_id: Option<i64>,
    pub workflow_node_run_id: Option<i64>,
    pub node_id: Option<String>,
    pub control_kind: Option<ControlKind>,
    pub message: Option<String>,
}

impl SchedulerControlClient {
    pub fn new(config: &Config) -> Result<Self, SendableError> {
        let transport = match config.scheduler_control_transport.as_str() {
            "disabled" => SchedulerControlTransport::Disabled,
            "http" => {
                let url =
                    reqwest::Url::parse(&config.scheduler_control_endpoint).map_err(|err| {
                        Box::new(RuntimeError::new(
                            "worker.scheduler_control.invalid_endpoint".into(),
                            err.to_string(),
                        )) as SendableError
                    })?;
                SchedulerControlTransport::Http(url)
            }
            "tcp" => SchedulerControlTransport::Tcp(config.scheduler_control_endpoint.clone()),
            other => {
                return Err(Box::new(RuntimeError::new(
                    "worker.scheduler_control.unknown_transport".into(),
                    format!("Unknown scheduler control transport '{other}'"),
                )));
            }
        };

        Ok(Self {
            worker_id: config.worker_id,
            transport,
            http_client: reqwest::Client::new(),
        })
    }

    pub async fn send(
        &self,
        kind: WorkerControlEventKind,
        details: EventDetails,
    ) -> Result<(), SendableError> {
        if matches!(&self.transport, SchedulerControlTransport::Disabled) {
            return Ok(());
        }

        let event = self.event(kind, details);
        self.send_event(event).await
    }

    fn event(&self, kind: WorkerControlEventKind, details: EventDetails) -> WorkerControlEvent {
        let mut event = WorkerControlEvent::new(
            self.worker_id.to_string(),
            kind,
            Utc::now().timestamp_millis(),
        );
        if let Some(workflow_run_id) = details.workflow_run_id {
            event = event.with_workflow_run_id(workflow_run_id);
        }
        if let Some(workflow_node_run_id) = details.workflow_node_run_id {
            event = event.with_workflow_node_run_id(workflow_node_run_id);
        }
        if let Some(node_id) = details.node_id {
            event = event.with_node_id(node_id);
        }
        if let Some(control_kind) = details.control_kind {
            event = event.with_control_kind(control_kind);
        }
        if let Some(message) = details.message {
            event = event.with_message(message);
        }
        event
    }

    async fn send_event(&self, event: WorkerControlEvent) -> Result<(), SendableError> {
        match &self.transport {
            SchedulerControlTransport::Disabled => Ok(()),
            SchedulerControlTransport::Http(base_url) => self.send_http(base_url, event).await,
            SchedulerControlTransport::Tcp(endpoint) => self.send_tcp(endpoint, event).await,
        }
    }

    async fn send_http(
        &self,
        base_url: &reqwest::Url,
        event: WorkerControlEvent,
    ) -> Result<(), SendableError> {
        let url = base_url.join("worker-control/events").map_err(|err| {
            Box::new(RuntimeError::new(
                "worker.scheduler_control.http_url".into(),
                err.to_string(),
            )) as SendableError
        })?;
        let body = encode_event(event)?;
        let response = self
            .http_client
            .post(url)
            .header(CONTENT_TYPE, "application/x-protobuf")
            .body(body)
            .send()
            .await
            .map_err(|err| {
                Box::new(RuntimeError::new(
                    "worker.scheduler_control.http_send".into(),
                    err.to_string(),
                )) as SendableError
            })?;
        let status = response.status();
        let body = response.bytes().await.map_err(|err| {
            Box::new(RuntimeError::new(
                "worker.scheduler_control.http_body".into(),
                err.to_string(),
            )) as SendableError
        })?;
        let ack = SchedulerControlAck::decode(body.as_ref()).map_err(|err| {
            Box::new(RuntimeError::new(
                "worker.scheduler_control.decode_ack".into(),
                err.to_string(),
            )) as SendableError
        })?;

        if status.is_success() && ack.accepted {
            return Ok(());
        }

        Err(Box::new(RuntimeError::new(
            "worker.scheduler_control.rejected".into(),
            format!("Scheduler rejected control event: {}", ack.message),
        )))
    }

    async fn send_tcp(
        &self,
        endpoint: &str,
        event: WorkerControlEvent,
    ) -> Result<(), SendableError> {
        let mut stream = TcpStream::connect(endpoint).await.map_err(|err| {
            Box::new(RuntimeError::new(
                "worker.scheduler_control.tcp_connect".into(),
                err.to_string(),
            )) as SendableError
        })?;
        let body = encode_event(event)?;
        stream
            .write_u32(body.len() as u32)
            .await
            .map_err(io_error("write_frame_len"))?;
        stream
            .write_all(&body)
            .await
            .map_err(io_error("write_frame"))?;

        let len = stream.read_u32().await.map_err(io_error("read_ack_len"))? as usize;
        if len > MAX_FRAME_BYTES {
            return Err(Box::new(RuntimeError::new(
                "worker.scheduler_control.ack_too_large".into(),
                format!("Ack length {len} exceeds maximum {MAX_FRAME_BYTES}"),
            )));
        }
        let mut ack_body = vec![0; len];
        stream
            .read_exact(&mut ack_body)
            .await
            .map_err(io_error("read_ack"))?;
        let ack = SchedulerControlAck::decode(ack_body.as_slice()).map_err(|err| {
            Box::new(RuntimeError::new(
                "worker.scheduler_control.decode_ack".into(),
                err.to_string(),
            )) as SendableError
        })?;

        if ack.accepted {
            return Ok(());
        }

        Err(Box::new(RuntimeError::new(
            "worker.scheduler_control.rejected".into(),
            format!("Scheduler rejected control event: {}", ack.message),
        )))
    }
}

impl EventDetails {
    pub fn empty() -> Self {
        Self {
            workflow_run_id: None,
            workflow_node_run_id: None,
            node_id: None,
            control_kind: None,
            message: None,
        }
    }

    pub fn for_action(
        workflow_run_id: i64,
        workflow_node_run_id: i64,
        node_id: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            workflow_run_id: Some(workflow_run_id),
            workflow_node_run_id: Some(workflow_node_run_id),
            node_id: Some(node_id.into()),
            control_kind: None,
            message: Some(message.into()),
        }
    }

    pub fn for_control(
        workflow_run_id: i64,
        control_kind: ControlKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            workflow_run_id: Some(workflow_run_id),
            workflow_node_run_id: None,
            node_id: None,
            control_kind: Some(control_kind),
            message: Some(message.into()),
        }
    }
}

fn encode_event(event: WorkerControlEvent) -> Result<Vec<u8>, SendableError> {
    let mut body = Vec::new();
    event.encode(&mut body).map_err(|err| {
        Box::new(RuntimeError::new(
            "worker.scheduler_control.encode_event".into(),
            err.to_string(),
        )) as SendableError
    })?;
    Ok(body)
}

fn io_error(context: &'static str) -> impl FnOnce(std::io::Error) -> SendableError {
    move |err| {
        Box::new(RuntimeError::new(
            format!("worker.scheduler_control.{context}"),
            err.to_string(),
        )) as SendableError
    }
}
