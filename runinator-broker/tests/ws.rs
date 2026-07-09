#![cfg(feature = "ws")]

use chrono::Utc;
use runinator_broker::{
    ws::{client::WsBroker, server::serve},
    Broker, BrokerMessage, ControlCommand, ResultMessage,
};
use runinator_comm::{ActionCommand, ControlKind, WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::json;
use runinator_models::workflows::WorkflowAction;
use std::time::Duration;
use tokio::net::TcpListener;
use uuid::Uuid;

async fn spawn_server() -> (tokio::task::JoinHandle<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = serve(listener, runinator_broker::in_memory::InMemoryBroker::new()).await;
    });
    (server, format!("ws://{addr}/"))
}

#[tokio::test]
async fn ws_broker_delivers_published_messages() {
    let (server, url) = spawn_server().await;
    let broker = WsBroker::connect(url, None);
    let message = BrokerMessage {
        command: action_command(),
        dedupe_key: Some("ws-test".into()),
        enqueued_at: Utc::now(),
    };

    broker.publish(message).await.unwrap();
    let delivery = tokio::time::timeout(Duration::from_secs(5), broker.receive("test-consumer"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.workflow_run_id, Uuid::from_u128(42));
    assert_eq!(delivery.command.workflow_node_run_id, Uuid::from_u128(99));
    assert_eq!(delivery.dedupe_key, "ws-test");
    broker
        .ack("test-consumer", delivery.delivery_id)
        .await
        .unwrap();

    server.abort();
}

#[tokio::test]
async fn ws_broker_delivers_control_messages() {
    let (server, url) = spawn_server().await;
    let broker = WsBroker::connect(url, None);

    broker
        .publish_control(ControlCommand::new(
            Uuid::from_u128(42),
            ControlKind::Cancel,
        ))
        .await
        .unwrap();
    let delivery = tokio::time::timeout(
        Duration::from_secs(5),
        broker.receive_control("test-consumer"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(delivery.command.workflow_run_id, Uuid::from_u128(42));
    assert!(matches!(delivery.command.kind, ControlKind::Cancel));
    broker
        .ack_control("test-consumer", delivery.delivery_id)
        .await
        .unwrap();

    server.abort();
}

#[tokio::test]
async fn ws_broker_delivers_result_events() {
    let (server, url) = spawn_server().await;
    let broker = WsBroker::connect(url, None);
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
            dedupe_key: Some("ws-result-test".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();
    let delivery = tokio::time::timeout(
        Duration::from_secs(5),
        broker.receive_result("result-consumer"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(delivery.event.workflow_run_id, Uuid::from_u128(42));
    assert_eq!(delivery.event.workflow_node_run_id, Uuid::from_u128(99));
    assert_eq!(delivery.dedupe_key, "ws-result-test");
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

/// proves the multiplexing model: one connection, one blocking `receive_for` with nothing matching
/// it yet (so it sits in the pending map indefinitely), racing a concurrent publish+receive+ack cycle
/// on the *same* client/connection. if requests serialized on the socket instead of being dispatched
/// independently, the fast cycle would hang behind the slow one instead of completing quickly.
#[tokio::test]
async fn ws_broker_concurrent_receive_for_does_not_block_concurrent_requests() {
    use runinator_broker::ConsumerProfile;

    let (server, url) = spawn_server().await;
    let broker = std::sync::Arc::new(WsBroker::connect(url, None));

    // never matches anything this test publishes, so it blocks for the lifetime of the test.
    let stuck_profile = ConsumerProfile::shared("stuck")
        .with_replica_id(Uuid::now_v7())
        .exclusive();
    let stuck = tokio::spawn({
        let broker = std::sync::Arc::clone(&broker);
        async move { broker.receive_for(&stuck_profile).await }
    });
    // give the blocking request time to actually be in flight (registered in the pending map)
    // before racing the fast cycle against it.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let command = action_command();
    let command_id = command.command_id;
    let fast_cycle = tokio::time::timeout(Duration::from_secs(5), async {
        broker
            .publish(BrokerMessage {
                command,
                dedupe_key: Some("ws-concurrency-test".into()),
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();
        let delivery = broker.receive("fast-consumer").await.unwrap();
        broker
            .ack("fast-consumer", delivery.delivery_id)
            .await
            .unwrap();
        delivery
    })
    .await
    .expect("fast publish/receive/ack cycle must not be blocked by the stuck receive_for");
    assert_eq!(fast_cycle.command.command_id, command_id);

    stuck.abort();
    server.abort();
}

/// regression: an inbound keepalive ping (from the server or any intermediary) must not be treated
/// as a disconnect. the client used to `break` on every non-text frame, so a single ping tore the
/// connection down and forced a reconnect — the churn behind the desktop-worker relay flapping. the
/// client must instead stay connected and answer the ping with a pong on the same connection.
#[tokio::test]
async fn ws_broker_answers_inbound_ping_instead_of_dropping() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        ws.send(Message::Ping(Vec::from(&b"keepalive"[..]).into()))
            .await
            .unwrap();
        loop {
            match ws.next().await {
                Some(Ok(Message::Pong(payload))) => {
                    assert_eq!(payload.as_ref(), b"keepalive");
                    break;
                }
                // ignore the client's own outbound keepalive pings, etc.
                Some(Ok(_)) => continue,
                other => panic!("expected a pong on the same connection, got {other:?}"),
            }
        }
    });

    let _broker = WsBroker::connect(format!("ws://{addr}/"), None);
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("client must answer the ping on the same connection, not reconnect")
        .unwrap();
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
            required_labels: Default::default(),
        },
        attempt: 1,
        parameters: json!({ "value": true }),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
    }
}
