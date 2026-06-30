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
        },
        dedupe_key: Some("http-test".into()),
        enqueued_at: Utc::now(),
    };

    broker.publish(message).await.unwrap();
    let delivery = broker.receive("test-consumer").await.unwrap();
    assert_eq!(delivery.command.workflow_run_id, Uuid::from_u128(42));
    assert_eq!(delivery.command.workflow_node_run_id, Uuid::from_u128(99));
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
    assert_eq!(delivery.event.workflow_run_id, Uuid::from_u128(42));
    assert_eq!(delivery.event.workflow_node_run_id, Uuid::from_u128(99));
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

    let run_id = Uuid::from_u128(7);
    broker
        .publish_event(EventMessage::new(UiEvent::WorkflowRunChanged { run_id }))
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
    assert!(matches!(a.event, UiEvent::WorkflowRunChanged { run_id: r } if r == run_id));
    assert!(matches!(b.event, UiEvent::WorkflowRunChanged { run_id: r } if r == run_id));

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
            required_labels: Default::default(),
        },
        attempt: 1,
        parameters: json!({ "value": true }),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
    }
}

fn bearer_client(token: &str) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

#[tokio::test]
async fn http_broker_auth_gates_and_scopes_by_replica() {
    use runinator_auth::AuthConfig;
    use runinator_broker::http::auth::BrokerAuth;
    use runinator_broker::http::server::serve_with_auth;
    use runinator_broker::ConsumerProfile;
    use runinator_comm::ActionTarget;

    let secret = b"broker-integration-secret".to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_with_auth(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
        Some(Arc::new(BrokerAuth::new(secret.clone(), None))),
    ));
    let base = Url::parse(&format!("http://{addr}/")).unwrap();

    // no token: every gated endpoint is rejected.
    let anon = HttpBroker::new(base.clone(), reqwest::Client::new());
    assert!(anon.receive("c").await.is_err());

    // a replica-scoped token authenticates and pins the consumer to its replica.
    let replica = Uuid::now_v7();
    let config = AuthConfig {
        enabled: true,
        jwt_secret: secret,
        jwt_secret_previous: None,
        access_ttl_secs: 60,
        refresh_ttl_secs: 60,
    };
    let (token, _) = runinator_auth::issue_replica_token(&config, Uuid::now_v7(), replica).unwrap();
    let authed = HttpBroker::new(base.clone(), bearer_client(&token));

    let mut command = action_command();
    command.target = ActionTarget::Replica {
        replica_id: replica,
    };
    authed
        .publish(BrokerMessage {
            command: command.clone(),
            dedupe_key: Some("auth-scope".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    // presenting a different replica id is forbidden, even with a valid token.
    let imposter = ConsumerProfile::shared("desktop")
        .with_replica_id(Uuid::now_v7())
        .exclusive();
    assert!(authed.receive_for(&imposter).await.is_err());

    // receiving for the token's own replica succeeds.
    let profile = ConsumerProfile::shared("desktop")
        .with_replica_id(replica)
        .exclusive();
    let delivery = authed.receive_for(&profile).await.unwrap();
    assert_eq!(delivery.command.command_id, command.command_id);

    server.abort();
}
