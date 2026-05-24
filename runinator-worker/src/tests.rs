use std::ffi::OsString;

use runinator_broker::{Broker, in_memory::InMemoryBroker};
use runinator_comm::{ActionCommand, WorkflowResultEventKind};
use runinator_models::workflows::{WorkflowAction, WorkflowStatus};
use serde_json::json;
use uuid::Uuid;

use crate::{output_sink::RunOutputSink, provider_service_url_fallback};

#[test]
fn provider_service_url_uses_api_base_url_when_env_is_missing() {
    assert_eq!(
        provider_service_url_fallback(None, "http://127.0.0.1:8080/"),
        Some(OsString::from("http://127.0.0.1:8080/"))
    );
}

#[test]
fn provider_service_url_preserves_existing_env() {
    assert_eq!(
        provider_service_url_fallback(
            Some(OsString::from("http://127.0.0.1:9090/")),
            "http://127.0.0.1:8080/",
        ),
        None
    );
}

#[test]
fn provider_service_url_replaces_empty_env() {
    assert_eq!(
        provider_service_url_fallback(Some(OsString::from("  ")), "http://127.0.0.1:8080/"),
        Some(OsString::from("http://127.0.0.1:8080/"))
    );
}

#[tokio::test]
async fn output_sink_publishes_result_events_to_broker() {
    let broker = std::sync::Arc::new(InMemoryBroker::new());
    let command = action_command();
    let sink = RunOutputSink::new(
        command.clone(),
        broker.clone(),
        tokio::runtime::Handle::current(),
    );

    sink.emit_log("hello".into());
    sink.flush().await.unwrap();
    sink.publish_status(
        WorkflowStatus::Succeeded,
        Some(json!({ "success": true })),
        Some("done".into()),
    )
    .await
    .unwrap();

    let chunk_delivery = broker.receive_result("test-ws").await.unwrap();
    assert_eq!(chunk_delivery.event.command_id, command.command_id);
    match chunk_delivery.event.kind {
        WorkflowResultEventKind::Chunk { chunk } => {
            assert_eq!(chunk.stream, "log");
            assert_eq!(chunk.content, "hello");
        }
        _ => panic!("expected chunk result event"),
    }

    let status_delivery = broker.receive_result("test-ws").await.unwrap();
    match status_delivery.event.kind {
        WorkflowResultEventKind::Status {
            status, message, ..
        } => {
            assert_eq!(status, WorkflowStatus::Succeeded);
            assert_eq!(message.as_deref(), Some("done"));
        }
        _ => panic!("expected status result event"),
    }
}

fn action_command() -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: 42,
        workflow_node_run_id: 99,
        node_id: "node-a".into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: json!({}),
            mcp_enabled: false,
            tags: Vec::new(),
        },
        attempt: 1,
        parameters: json!({}),
    }
}
