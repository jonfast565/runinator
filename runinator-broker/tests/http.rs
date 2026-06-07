use chrono::Utc;
use reqwest::Url;
use runinator_broker::{
    http::{client::HttpBroker, server::serve},
    Broker, BrokerMessage, ControlCommand, EventMessage, ResultMessage,
};
use runinator_comm::{
    ActionCommand, ControlKind, UiEvent, WorkflowResultEvent, WorkflowResultEventKind,
};
use runinator_models::json;
use runinator_models::workflows::WorkflowAction;
use std::sync::Arc;
use std::time::Duration;
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
                configuration: runinator_models::workflows::WorkflowObject::default(),
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

#[tokio::test]
async fn http_broker_delivers_result_events() {
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
            dedupe_key: Some("http-result-test".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();
    let delivery = broker.receive_result("result-consumer").await.unwrap();
    assert_eq!(delivery.event.workflow_run_id, 42);
    assert_eq!(delivery.event.workflow_node_run_id, 99);
    assert_eq!(delivery.dedupe_key, "http-result-test");
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
async fn http_broker_fans_out_events_to_every_subscriber() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
    ));
    let broker = Arc::new(HttpBroker::new(
        Url::parse(&format!("http://{addr}/")).unwrap(),
        reqwest::Client::new(),
    ));

    // both replicas start receiving (and so subscribe) before the event is published.
    let a = tokio::spawn({
        let broker = Arc::clone(&broker);
        async move { broker.receive_event("ws-a").await }
    });
    let b = tokio::spawn({
        let broker = Arc::clone(&broker);
        async move { broker.receive_event("ws-b").await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    broker
        .publish_event(EventMessage::new(UiEvent::WorkflowRunChanged { run_id: 7 }))
        .await
        .unwrap();

    let a = tokio::time::timeout(Duration::from_secs(2), a)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let b = tokio::time::timeout(Duration::from_secs(2), b)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(a.event, UiEvent::WorkflowRunChanged { run_id: 7 }));
    assert!(matches!(b.event, UiEvent::WorkflowRunChanged { run_id: 7 }));

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
            configuration: runinator_models::workflows::WorkflowObject::default(),
            mcp_enabled: false,
            tags: Vec::new(),
        },
        attempt: 1,
        parameters: json!({ "value": true }),
    }
}
