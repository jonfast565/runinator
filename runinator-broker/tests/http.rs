use chrono::Utc;
use reqwest::Url;
use runinator_broker::{
    http::{client::HttpBroker, server::serve},
    Broker, ControlCommand, EffectMessage, EffectResultMessage, EventMessage,
};
use runinator_comm::{
    ActionTarget, AgentCommand, AgentDirectiveKind, ConsumerProfile, ControlKind, EffectCommand,
    EffectExecutor, UiEvent,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use uuid::Uuid;

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
async fn http_broker_delivers_targeted_agent_directives() {
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
    let replica_id = Uuid::now_v7();
    broker
        .publish_agent(agent_command(replica_id))
        .await
        .unwrap();
    let profile = ConsumerProfile::shared("agent-test").with_replica_id(replica_id);
    let delivery = broker.receive_agent_for(&profile).await.unwrap();
    assert_eq!(delivery.command.replica_id, replica_id);
    broker
        .ack_agent(&profile.id, delivery.delivery_id)
        .await
        .unwrap();
    server.abort();
}

#[tokio::test]
async fn http_broker_round_trips_executor_routed_effects() {
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
    assert_effect_round_trip(&broker).await;
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
        .publish_event(EventMessage::new(UiEvent::new(
            None,
            runinator_comm::UiEventKind::WorkflowRunChanged { run_id },
        )))
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
    assert!(matches!(
        a.event.kind,
        runinator_comm::UiEventKind::WorkflowRunChanged { run_id: r } if r == run_id
    ));
    assert!(matches!(
        b.event.kind,
        runinator_comm::UiEventKind::WorkflowRunChanged { run_id: r } if r == run_id
    ));

    server.abort();
}

fn agent_command(replica_id: Uuid) -> AgentCommand {
    AgentCommand {
        directive_id: Uuid::now_v7(),
        replica_id,
        target: ActionTarget::Replica { replica_id },
        kind: AgentDirectiveKind::Diagnostics,
        issued_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::minutes(5),
    }
}

async fn assert_effect_round_trip(broker: &dyn Broker) {
    let provider = effect_command(EffectExecutor::Provider);
    let infrastructure = effect_command(EffectExecutor::Infrastructure);
    for command in [provider.clone(), infrastructure.clone()] {
        broker
            .publish_effect(EffectMessage {
                command,
                dedupe_key: None,
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();
    }
    let delivery = broker.receive_effect("provider").await.unwrap();
    assert_eq!(delivery.command.effect_id, provider.effect_id);
    broker
        .ack_effect("provider", delivery.delivery_id)
        .await
        .unwrap();
    let delivery = broker
        .receive_infrastructure_effect("infrastructure")
        .await
        .unwrap();
    assert_eq!(delivery.command.effect_id, infrastructure.effect_id);
    broker
        .ack_effect("infrastructure", delivery.delivery_id)
        .await
        .unwrap();
    let result = runinator_comm::EffectResult::status(
        &provider,
        runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
        None,
        None,
    );
    broker
        .publish_effect_result(EffectResultMessage {
            result: result.clone(),
            dedupe_key: None,
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();
    let delivery = broker.receive_effect_result("engine").await.unwrap();
    assert_eq!(delivery.result.event_id, result.event_id);
    broker
        .ack_effect_result("engine", delivery.delivery_id)
        .await
        .unwrap();
}

fn effect_command(executor: EffectExecutor) -> EffectCommand {
    EffectCommand {
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt: 0,
        request: runinator_models::workflow_vm::WorkflowEffectRequest::Timer { due_at: 42 },
        executor,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: Uuid::now_v7().to_string(),
        notification_delivery_id: None,
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
    assert!(anon.receive_effect("c").await.is_err());

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

    let mut command = effect_command(EffectExecutor::Provider);
    command.target = ActionTarget::Replica {
        replica_id: replica,
    };
    authed
        .publish_effect(EffectMessage {
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
    assert!(authed.receive_effect_for(&imposter).await.is_err());

    // receiving for the token's own replica succeeds.
    let profile = ConsumerProfile::shared("desktop")
        .with_replica_id(replica)
        .exclusive();
    let delivery = authed.receive_effect_for(&profile).await.unwrap();
    assert_eq!(delivery.command.command_id, command.command_id);

    server.abort();
}
