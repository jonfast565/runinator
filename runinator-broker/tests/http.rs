use chrono::Utc;
use reqwest::Url;
use runinator_broker::{
    http::{client::HttpBroker, server::serve},
    Broker, BrokerMessage, ControlCommand,
};
use runinator_comm::{ActionCommand, ControlKind};
use runinator_models::workflows::WorkflowAction;
use serde_json::json;
use tokio::net::TcpListener;
use uuid::Uuid;

#[tokio::test]
async fn http_broker_delivers_published_messages() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
    ));
    let broker = HttpBroker::new(
        Url::parse(&format!("http://{addr}/")).unwrap(),
        reqwest::Client::new(),
    );
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
        dedupe_key: Some("http-test".into()),
        enqueued_at: Utc::now(),
    };

    broker.publish(message).await.unwrap();
    let delivery = broker.receive("test-consumer").await.unwrap();
    assert_eq!(delivery.command.workflow_run_id, 42);
    assert_eq!(delivery.command.workflow_node_run_id, 99);
    assert_eq!(delivery.dedupe_key, "http-test");
    broker
        .ack("test-consumer", delivery.delivery_id)
        .await
        .unwrap();

    server.abort();
}

#[tokio::test]
async fn http_broker_delivers_control_messages() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
    ));
    let broker = HttpBroker::new(
        Url::parse(&format!("http://{addr}/")).unwrap(),
        reqwest::Client::new(),
    );

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
