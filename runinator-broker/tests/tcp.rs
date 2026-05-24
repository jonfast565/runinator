use chrono::Utc;
use runinator_broker::{
    tcp::{client::TcpBroker, server::serve},
    Broker, BrokerMessage, ControlCommand, ResultMessage,
};
use runinator_comm::{ActionCommand, ControlKind, WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::workflows::WorkflowAction;
use serde_json::json;
use tokio::net::TcpListener;
use uuid::Uuid;

#[tokio::test]
async fn tcp_broker_delivers_published_messages() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
    ));
    let broker = TcpBroker::new(addr.to_string());
    let message = BrokerMessage {
        command: ActionCommand {
            command_id: Uuid::new_v4(),
            workflow_run_id: 42,
            workflow_node_run_id: 99,
            node_id: "run".into(),
            action: WorkflowAction {
                provider: "test".into(),
                function: "execute".into(),
                timeout_seconds: 60,
                configuration: json!({}),
                mcp_enabled: false,
                tags: Vec::new(),
            },
            attempt: 1,
            parameters: json!({ "value": true }),
        },
        dedupe_key: Some("tcp-test".into()),
        enqueued_at: Utc::now(),
    };

    broker.publish(message).await.unwrap();
    let delivery = broker.receive("test-consumer").await.unwrap();
    assert_eq!(delivery.command.workflow_run_id, 42);
    assert_eq!(delivery.command.workflow_node_run_id, 99);
    assert_eq!(delivery.dedupe_key, "tcp-test");
    broker
        .ack("test-consumer", delivery.delivery_id)
        .await
        .unwrap();

    server.abort();
}

#[tokio::test]
async fn tcp_broker_delivers_control_messages() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
    ));
    let broker = TcpBroker::new(addr.to_string());

    broker
        .publish_control(ControlCommand::new(42, ControlKind::Cancel))
        .await
        .unwrap();
    let delivery = broker.receive_control("test-consumer").await.unwrap();
    assert_eq!(delivery.command.workflow_run_id, 42);
    assert!(matches!(delivery.command.kind, ControlKind::Cancel));
    broker
        .ack_control("test-consumer", delivery.delivery_id)
        .await
        .unwrap();

    server.abort();
}

#[tokio::test]
async fn tcp_broker_delivers_result_events() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
    ));
    let broker = TcpBroker::new(addr.to_string());
    let command = action_command();
    let event = WorkflowResultEvent::chunk(
        &command,
        runinator_models::runs::NewRunChunk {
            stream: "log".into(),
            content: "hello".into(),
        },
    );

    broker
        .publish_result(ResultMessage {
            event,
            dedupe_key: Some("tcp-result-test".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();
    let delivery = broker.receive_result("result-consumer").await.unwrap();
    assert_eq!(delivery.event.workflow_run_id, 42);
    assert_eq!(delivery.event.workflow_node_run_id, 99);
    assert_eq!(delivery.dedupe_key, "tcp-result-test");
    match delivery.event.kind {
        WorkflowResultEventKind::Chunk { chunk } => assert_eq!(chunk.content, "hello"),
        _ => panic!("expected chunk event"),
    }
    broker
        .ack_result("result-consumer", delivery.delivery_id)
        .await
        .unwrap();

    server.abort();
}

fn action_command() -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: 42,
        workflow_node_run_id: 99,
        node_id: "run".into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: json!({}),
            mcp_enabled: false,
            tags: Vec::new(),
        },
        attempt: 1,
        parameters: json!({ "value": true }),
    }
}
