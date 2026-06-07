use chrono::Utc;
use runinator_broker::{
    tcp::{client::TcpBroker, server::serve},
    Broker, BrokerMessage, ControlCommand, ResultMessage,
};
use runinator_comm::{ActionCommand, ControlKind, WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::json;
use runinator_models::workflows::WorkflowAction;
use std::time::Duration;
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
            workflow_run_id: Uuid::from_u128(42),
            workflow_node_run_id: Uuid::from_u128(99),
            node_id: "run".into(),
            action: WorkflowAction {
                provider: "test".into(),
                function: "execute".into(),
                timeout_seconds: 60,
                configuration: runinator_models::workflows::WorkflowObject::default(),
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
    assert_eq!(delivery.command.workflow_run_id, Uuid::from_u128(42));
    assert_eq!(delivery.command.workflow_node_run_id, Uuid::from_u128(99));
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
        .publish_control(ControlCommand::new(
            Uuid::from_u128(42),
            ControlKind::Cancel,
        ))
        .await
        .unwrap();
    let delivery = broker.receive_control("test-consumer").await.unwrap();
    assert_eq!(delivery.command.workflow_run_id, Uuid::from_u128(42));
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
    assert_eq!(delivery.event.workflow_run_id, Uuid::from_u128(42));
    assert_eq!(delivery.event.workflow_node_run_id, Uuid::from_u128(99));
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

#[tokio::test]
async fn tcp_broker_times_out_publish_response() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let broker = TcpBroker::with_timeout(addr.to_string(), Duration::from_millis(25));

    let err = broker
        .publish(BrokerMessage {
            command: action_command(),
            dedupe_key: Some("tcp-timeout-test".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .expect_err("publish should time out waiting for a response");

    assert!(err.to_string().contains("timed out"));
    server.abort();
}

fn action_command() -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: Uuid::from_u128(42),
        workflow_node_run_id: Uuid::from_u128(99),
        node_id: "run".into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: runinator_models::workflows::WorkflowObject::default(),
            mcp_enabled: false,
            tags: Vec::new(),
        },
        attempt: 1,
        parameters: json!({ "value": true }),
    }
}
