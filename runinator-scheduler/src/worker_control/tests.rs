use std::sync::Arc;

use async_trait::async_trait;
use prost::Message;
use runinator_comm::{
    ControlKind,
    worker_control::{SchedulerControlAck, WorkerControlEvent, WorkerControlEventKind},
};
use runinator_models::errors::SendableError;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Notify},
};

use super::{WorkerControlApi, handle_event, serve_http, serve_tcp};

#[derive(Default)]
struct RecordingApi {
    controls: Mutex<Vec<(i64, ControlKind)>>,
}

#[async_trait]
impl WorkerControlApi for RecordingApi {
    async fn pause_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        self.controls
            .lock()
            .await
            .push((workflow_run_id, ControlKind::Pause));
        Ok(())
    }

    async fn resume_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        self.controls
            .lock()
            .await
            .push((workflow_run_id, ControlKind::Resume));
        Ok(())
    }

    async fn cancel_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        self.controls
            .lock()
            .await
            .push((workflow_run_id, ControlKind::Cancel));
        Ok(())
    }
}

#[tokio::test]
async fn http_event_submit_returns_ack() {
    let api = Arc::new(RecordingApi::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(Notify::new());
    let server_api: Arc<dyn WorkerControlApi> = api.clone();
    let server = tokio::spawn(serve_http(listener, server_api, shutdown.clone()));

    let event = lifecycle_event(WorkerControlEventKind::WorkerStarted);
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/worker-control/events"))
        .header("content-type", "application/x-protobuf")
        .body(encode(event))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let ack = SchedulerControlAck::decode(response.bytes().await.unwrap()).unwrap();
    assert!(ack.accepted);

    shutdown.notify_waiters();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn http_invalid_payload_returns_structured_failure() {
    let api = Arc::new(RecordingApi::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(Notify::new());
    let server_api: Arc<dyn WorkerControlApi> = api.clone();
    let server = tokio::spawn(serve_http(listener, server_api, shutdown.clone()));

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/worker-control/events"))
        .header("content-type", "application/x-protobuf")
        .body(vec![0xff, 0xff, 0xff])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let ack = SchedulerControlAck::decode(response.bytes().await.unwrap()).unwrap();
    assert!(!ack.accepted);
    assert!(ack.message.contains("Invalid protobuf payload"));

    shutdown.notify_waiters();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn tcp_event_submit_returns_ack() {
    let api = Arc::new(RecordingApi::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(Notify::new());
    let server_api: Arc<dyn WorkerControlApi> = api.clone();
    let server = tokio::spawn(serve_tcp(listener, server_api, shutdown.clone()));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let body = encode(lifecycle_event(WorkerControlEventKind::WorkerStopping));
    stream.write_u32(body.len() as u32).await.unwrap();
    stream.write_all(&body).await.unwrap();
    let ack_len = stream.read_u32().await.unwrap() as usize;
    let mut ack_body = vec![0; ack_len];
    stream.read_exact(&mut ack_body).await.unwrap();
    let ack = SchedulerControlAck::decode(ack_body.as_slice()).unwrap();
    assert!(ack.accepted);

    shutdown.notify_waiters();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn control_requested_maps_to_shared_control_kind() {
    let api = RecordingApi::default();
    let event = WorkerControlEvent::new("worker-1", WorkerControlEventKind::ControlRequested, 1)
        .with_workflow_run_id(44)
        .with_control_kind(ControlKind::Cancel);

    let ack = handle_event(&api, event).await;

    assert!(ack.accepted);
    assert_eq!(
        api.controls.lock().await.as_slice(),
        &[(44, ControlKind::Cancel)]
    );
}

fn lifecycle_event(kind: WorkerControlEventKind) -> WorkerControlEvent {
    WorkerControlEvent::new("worker-1", kind, 1)
}

fn encode(event: WorkerControlEvent) -> Vec<u8> {
    let mut body = Vec::new();
    event.encode(&mut body).unwrap();
    body
}
