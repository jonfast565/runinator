use chrono::Utc;
use runinator_broker::{
    tcp::{client::TcpBroker, server::serve},
    Broker, BrokerMessage, ControlCommand, EffectMessage, EffectResultMessage, ResultMessage,
};
use runinator_comm::{
    ActionCommand, ActionTarget, AgentCommand, AgentDirectiveKind, ConsumerProfile, ControlKind,
    EffectCommand, EffectExecutor, WorkflowResultEvent, WorkflowResultEventKind,
};
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
                required_labels: Default::default(),
                idempotency_key: None,
                function_binding: None,
            },
            attempt: 1,
            parameters: json!({ "value": true }),
            target: Default::default(),
            trace_id: Uuid::nil(),
            trace_context: Default::default(),
            notification_delivery_id: None,
            invocation_call_id: None,
            task_run_id: None,
            idempotency_key: None,
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
async fn tcp_broker_delivers_targeted_agent_directives() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
    ));
    let broker = TcpBroker::new(addr.to_string());
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
async fn tcp_broker_round_trips_executor_routed_effects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        runinator_broker::in_memory::InMemoryBroker::new(),
    ));
    let broker = TcpBroker::new(addr.to_string());
    assert_effect_round_trip(&broker).await;
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
            required_labels: Default::default(),
            idempotency_key: None,
            function_binding: None,
        },
        attempt: 1,
        parameters: json!({ "value": true }),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        notification_delivery_id: None,
        invocation_call_id: None,
        task_run_id: None,
        idempotency_key: None,
    }
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
    let provider_delivery = broker.receive_effect("provider").await.unwrap();
    assert_eq!(provider_delivery.command.effect_id, provider.effect_id);
    broker
        .ack_effect("provider", provider_delivery.delivery_id)
        .await
        .unwrap();
    let infrastructure_delivery = broker
        .receive_infrastructure_effect("infrastructure")
        .await
        .unwrap();
    assert_eq!(
        infrastructure_delivery.command.effect_id,
        infrastructure.effect_id
    );
    broker
        .ack_effect("infrastructure", infrastructure_delivery.delivery_id)
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
