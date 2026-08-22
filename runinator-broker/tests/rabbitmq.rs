#![cfg(feature = "rabbitmq")]

use chrono::Utc;
use runinator_broker::{
    adapters::rabbitmq::{RabbitMqBroker, RabbitMqBrokerConfig},
    ActionTarget, Broker, ConsumerProfile, ControlCommand, EffectMessage, EffectResultMessage,
};
use runinator_comm::{
    AgentCommand, AgentDirectiveKind, ControlKind, EffectCommand, EffectExecutor, EffectResult,
    EffectResultKind,
};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

async fn rabbitmq_broker() -> Option<RabbitMqBroker> {
    let uri = match std::env::var("RUNINATOR_RABBITMQ_URI") {
        Ok(uri) => uri,
        Err(_) => {
            eprintln!("skipping rabbitmq integration test; set RUNINATOR_RABBITMQ_URI");
            return None;
        }
    };
    let control_queue = std::env::var("RUNINATOR_RABBITMQ_CONTROL_QUEUE")
        .unwrap_or_else(|_| format!("runinator.test.control.{}", Uuid::new_v4()));
    let agent_prefix = std::env::var("RUNINATOR_RABBITMQ_AGENT_QUEUE_PREFIX")
        .unwrap_or_else(|_| format!("runinator.test.agent.{}", Uuid::new_v4()));
    let effect_queue = format!("runinator.test.effects.{}", Uuid::new_v4());
    let infrastructure_effect_queue = format!("{effect_queue}.infrastructure");
    let effect_result_queue = format!("{effect_queue}.results");

    Some(
        RabbitMqBroker::connect(
            RabbitMqBrokerConfig::new(uri)
                .with_control_queue(control_queue)
                .with_agent_queue_prefix(agent_prefix)
                .with_effect_queues(
                    effect_queue,
                    infrastructure_effect_queue,
                    effect_result_queue,
                )
                .with_client_id(format!("runinator-test-{}", Uuid::new_v4())),
        )
        .await
        .unwrap(),
    )
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_routes_agent_directives_by_replica_queue() {
    let Some(broker) = rabbitmq_broker().await else {
        return;
    };
    let replica_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();
    broker
        .publish_agent(agent_command(replica_id))
        .await
        .unwrap();
    let other = ConsumerProfile::shared(format!("agent-{other_id}")).with_replica_id(other_id);
    assert!(
        timeout(Duration::from_millis(300), broker.receive_agent_for(&other))
            .await
            .is_err()
    );
    let profile =
        ConsumerProfile::shared(format!("agent-{replica_id}")).with_replica_id(replica_id);
    let delivery = timeout(Duration::from_secs(10), broker.receive_agent_for(&profile))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.replica_id, replica_id);
    broker
        .ack_agent(&profile.id, delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_delivers_control_messages() {
    let Some(broker) = rabbitmq_broker().await else {
        return;
    };
    broker
        .publish_control(ControlCommand::new(
            Uuid::from_u128(4242),
            ControlKind::Cancel,
        ))
        .await
        .unwrap();

    let consumer = format!("test-control-{}", Uuid::new_v4());
    let delivery = timeout(Duration::from_secs(10), broker.receive_control(&consumer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.workflow_run_id, Uuid::from_u128(4242));
    assert!(matches!(delivery.command.kind, ControlKind::Cancel));
    broker
        .ack_control(&consumer, delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_nack_redelivers_messages() {
    let Some(broker) = rabbitmq_broker().await else {
        return;
    };
    let command = effect_command(EffectExecutor::Provider);
    let command_id = command.command_id;
    let consumer = format!("test-nack-{}", Uuid::new_v4());
    broker
        .publish_effect(EffectMessage {
            command,
            dedupe_key: Some(command_id.to_string()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let delivery = timeout(Duration::from_secs(10), broker.receive_effect(&consumer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.command_id, command_id);
    broker
        .nack_effect(&consumer, delivery.delivery_id)
        .await
        .unwrap();

    let redelivery = timeout(Duration::from_secs(10), broker.receive_effect(&consumer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(redelivery.command.command_id, command_id);
    broker
        .ack_effect(&consumer, redelivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_round_trips_executor_routed_effects() {
    let Some(broker) = rabbitmq_broker().await else {
        return;
    };
    assert_effect_round_trip(&broker).await;
}

async fn assert_effect_round_trip(broker: &dyn Broker) {
    let provider = effect_command(EffectExecutor::Provider);
    let infrastructure = effect_command(EffectExecutor::Infrastructure);
    for command in [&provider, &infrastructure] {
        broker
            .publish_effect(EffectMessage {
                command: command.clone(),
                dedupe_key: Some(command.effect_id.to_string()),
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();
    }
    let provider_profile = ConsumerProfile::shared(format!("effects-provider-{}", Uuid::new_v4()));
    let provider_delivery = timeout(
        Duration::from_secs(10),
        broker.receive_effect_for(&provider_profile),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(provider_delivery.command.effect_id, provider.effect_id);
    broker
        .nack_effect(&provider_profile.id, provider_delivery.delivery_id)
        .await
        .unwrap();
    let provider_redelivery = timeout(
        Duration::from_secs(10),
        broker.receive_effect_for(&provider_profile),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(provider_redelivery.command.effect_id, provider.effect_id);
    broker
        .ack_effect(&provider_profile.id, provider_redelivery.delivery_id)
        .await
        .unwrap();

    let infrastructure_consumer = format!("effects-infrastructure-{}", Uuid::new_v4());
    let infrastructure_delivery = timeout(
        Duration::from_secs(10),
        broker.receive_infrastructure_effect(&infrastructure_consumer),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        infrastructure_delivery.command.effect_id,
        infrastructure.effect_id
    );
    broker
        .ack_effect(
            &infrastructure_consumer,
            infrastructure_delivery.delivery_id,
        )
        .await
        .unwrap();

    let result = EffectResult {
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        event_id: Uuid::now_v7(),
        effect_id: provider.effect_id,
        workflow_run_id: provider.workflow_run_id,
        continuation_id: provider.continuation_id,
        attempt: provider.attempt,
        kind: EffectResultKind::Status {
            status: runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
            output: None,
            message: None,
        },
        timestamp: Utc::now(),
        trace_id: Uuid::now_v7(),
        notification_delivery_id: None,
    };
    broker
        .publish_effect_result(EffectResultMessage {
            result: result.clone(),
            dedupe_key: Some(result.event_id.to_string()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();
    let consumer = format!("effect-results-{}", Uuid::new_v4());
    let delivery = timeout(
        Duration::from_secs(10),
        broker.receive_effect_result(&consumer),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(delivery.result.event_id, result.event_id);
    broker
        .ack_effect_result(&consumer, delivery.delivery_id)
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

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_still_delivers_any_targeted_effects_via_receive_effect_for() {
    let Some(broker) = rabbitmq_broker().await else {
        return;
    };
    let command = effect_command(EffectExecutor::Provider);
    let command_id = command.command_id;
    broker
        .publish_effect(EffectMessage {
            command,
            dedupe_key: Some(command_id.to_string()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    // a plain, non-exclusive, unlabeled consumer still gets `Any` work through the shared queue.
    let profile = ConsumerProfile::shared(format!("test-any-{}", Uuid::new_v4()));
    let delivery = timeout(Duration::from_secs(10), broker.receive_effect_for(&profile))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.command_id, command_id);
    broker
        .ack_effect(&profile.id, delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_routes_labels_target_to_the_matching_consumer_only() {
    let Some(broker) = rabbitmq_broker().await else {
        return;
    };
    let mut command = effect_command(EffectExecutor::Provider);
    command.target = ActionTarget::Labels {
        selector: [("runner".to_string(), "creds-sync".to_string())].into(),
    };
    let command_id = command.command_id;
    broker
        .publish_effect(EffectMessage {
            command,
            dedupe_key: Some(command_id.to_string()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    // an exclusive consumer whose labels don't include `runner=creds-sync` must never see this
    // delivery (it would otherwise nack-and-loop on it forever since nothing else is competing).
    let mismatched = ConsumerProfile::shared(format!("test-mismatch-{}", Uuid::new_v4()))
        .with_labels([("runner".to_string(), "other".to_string())].into())
        .exclusive();
    let matching = ConsumerProfile::shared(format!("test-match-{}", Uuid::new_v4()))
        .with_labels([("runner".to_string(), "creds-sync".to_string())].into())
        .exclusive();

    // race both; only `matching` should ever resolve to this command, and it must resolve well
    // within the timeout even with `mismatched` also competing on the same targeted queue.
    let delivery = timeout(Duration::from_secs(10), async {
        tokio::select! {
            delivery = broker.receive_effect_for(&matching) => delivery,
            // if the mismatched profile somehow won the race, that's the bug under test: surface
            // it as a wrong delivery rather than hanging.
            delivery = broker.receive_effect_for(&mismatched) => delivery.map(|d| {
                panic!(
                    "mismatched consumer received command {} not intended for it",
                    d.command.command_id
                )
            }),
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(delivery.command.command_id, command_id);
    broker
        .ack_effect(&matching.id, delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_routes_replica_target_to_the_bound_replica_only() {
    let Some(broker) = rabbitmq_broker().await else {
        return;
    };
    let replica_id = Uuid::now_v7();
    let mut command = effect_command(EffectExecutor::Provider);
    command.target = ActionTarget::Replica { replica_id };
    let command_id = command.command_id;
    broker
        .publish_effect(EffectMessage {
            command,
            dedupe_key: Some(command_id.to_string()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let bound = ConsumerProfile::shared(format!("test-bound-{}", Uuid::new_v4()))
        .with_replica_id(replica_id)
        .exclusive();
    let delivery = timeout(Duration::from_secs(10), broker.receive_effect_for(&bound))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.command_id, command_id);
    broker
        .ack_effect(&bound.id, delivery.delivery_id)
        .await
        .unwrap();
}

fn agent_command(replica_id: Uuid) -> AgentCommand {
    AgentCommand {
        directive_id: Uuid::new_v4(),
        replica_id,
        target: ActionTarget::Replica { replica_id },
        kind: AgentDirectiveKind::Diagnostics,
        issued_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::minutes(5),
    }
}
