#![cfg(feature = "kafka")]

use chrono::Utc;
use runinator_broker::{
    adapters::kafka::{KafkaBroker, KafkaBrokerConfig},
    Broker, ControlCommand, EffectMessage, EffectResultMessage,
};
use runinator_comm::{
    ActionTarget, AgentCommand, AgentDirectiveKind, ConsumerProfile, ControlKind, EffectCommand,
    EffectExecutor, EffectResult, EffectResultKind,
};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

fn kafka_broker() -> Option<KafkaBroker> {
    let bootstrap = match std::env::var("RUNINATOR_KAFKA_BOOTSTRAP") {
        Ok(bootstrap) => bootstrap,
        Err(_) => {
            eprintln!("skipping kafka integration test; set RUNINATOR_KAFKA_BOOTSTRAP");
            return None;
        }
    };
    let control_topic = std::env::var("RUNINATOR_KAFKA_CONTROL_TOPIC")
        .unwrap_or_else(|_| "runinator.control".into());
    let agent_topic =
        std::env::var("RUNINATOR_KAFKA_AGENT_TOPIC").unwrap_or_else(|_| "runinator.agent".into());
    let effect_topic = std::env::var("RUNINATOR_KAFKA_EFFECT_TOPIC")
        .unwrap_or_else(|_| "runinator.effects".into());
    let infrastructure_effect_topic = std::env::var("RUNINATOR_KAFKA_INFRASTRUCTURE_EFFECT_TOPIC")
        .unwrap_or_else(|_| "runinator.effects.infrastructure".into());
    let effect_result_topic = std::env::var("RUNINATOR_KAFKA_EFFECT_RESULT_TOPIC")
        .unwrap_or_else(|_| "runinator.effect-results".into());

    Some(
        KafkaBroker::new(
            KafkaBrokerConfig::new(bootstrap)
                .with_control_topic(control_topic)
                .with_agent_topic(agent_topic)
                .with_effect_topics(
                    effect_topic,
                    infrastructure_effect_topic,
                    effect_result_topic,
                )
                .with_client_id(format!("runinator-test-{}", Uuid::new_v4())),
        )
        .unwrap(),
    )
}

#[tokio::test]
#[ignore = "requires a reachable Kafka broker and pre-created topics"]
async fn kafka_broker_delivers_targeted_agent_directives() {
    let Some(broker) = kafka_broker() else {
        return;
    };
    let replica_id = Uuid::new_v4();
    let directive_id = Uuid::new_v4();
    broker
        .publish_agent(agent_command(replica_id, directive_id))
        .await
        .unwrap();
    let profile =
        ConsumerProfile::shared(format!("agent-{replica_id}")).with_replica_id(replica_id);
    loop {
        let delivery = timeout(Duration::from_secs(10), broker.receive_agent_for(&profile))
            .await
            .unwrap()
            .unwrap();
        broker
            .ack_agent(&profile.id, delivery.delivery_id)
            .await
            .unwrap();
        if delivery.command.directive_id == directive_id {
            break;
        }
    }
}

#[tokio::test]
#[ignore = "requires a reachable Kafka broker and pre-created topics"]
async fn kafka_broker_delivers_control_messages() {
    let Some(broker) = kafka_broker() else {
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
    loop {
        let delivery = timeout(Duration::from_secs(10), broker.receive_control(&consumer))
            .await
            .unwrap()
            .unwrap();
        broker
            .ack_control(&consumer, delivery.delivery_id)
            .await
            .unwrap();
        if delivery.command.workflow_run_id == Uuid::from_u128(4242) {
            assert!(matches!(delivery.command.kind, ControlKind::Cancel));
            break;
        }
    }
}

#[tokio::test]
#[ignore = "requires a reachable Kafka broker and pre-created topics"]
async fn kafka_broker_nack_redelivers_messages() {
    let Some(broker) = kafka_broker() else {
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

    let delivery = loop {
        let delivery = timeout(Duration::from_secs(10), broker.receive_effect(&consumer))
            .await
            .unwrap()
            .unwrap();
        if delivery.command.command_id == command_id {
            break delivery;
        }
        broker
            .ack_effect(&consumer, delivery.delivery_id)
            .await
            .unwrap();
    };
    broker
        .nack_effect(&consumer, delivery.delivery_id)
        .await
        .unwrap();

    let redelivery = loop {
        let delivery = timeout(Duration::from_secs(10), broker.receive_effect(&consumer))
            .await
            .unwrap()
            .unwrap();
        if delivery.command.command_id == command_id {
            break delivery;
        }
        broker
            .ack_effect(&consumer, delivery.delivery_id)
            .await
            .unwrap();
    };
    broker
        .ack_effect(&consumer, redelivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable Kafka broker and pre-created topics"]
async fn kafka_broker_round_trips_executor_routed_effects() {
    let Some(broker) = kafka_broker() else {
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
    let provider_delivery = loop {
        let delivery = timeout(
            Duration::from_secs(10),
            broker.receive_effect_for(&provider_profile),
        )
        .await
        .unwrap()
        .unwrap();
        if delivery.command.effect_id == provider.effect_id {
            break delivery;
        }
        broker
            .ack_effect(&provider_profile.id, delivery.delivery_id)
            .await
            .unwrap();
    };
    broker
        .nack_effect(&provider_profile.id, provider_delivery.delivery_id)
        .await
        .unwrap();
    let provider_redelivery = loop {
        let delivery = timeout(
            Duration::from_secs(10),
            broker.receive_effect_for(&provider_profile),
        )
        .await
        .unwrap()
        .unwrap();
        if delivery.command.effect_id == provider.effect_id {
            break delivery;
        }
        broker
            .ack_effect(&provider_profile.id, delivery.delivery_id)
            .await
            .unwrap();
    };
    broker
        .ack_effect(&provider_profile.id, provider_redelivery.delivery_id)
        .await
        .unwrap();

    let infrastructure_consumer = format!("effects-infrastructure-{}", Uuid::new_v4());
    let infrastructure_delivery = loop {
        let delivery = timeout(
            Duration::from_secs(10),
            broker.receive_infrastructure_effect(&infrastructure_consumer),
        )
        .await
        .unwrap()
        .unwrap();
        if delivery.command.effect_id == infrastructure.effect_id {
            break delivery;
        }
        broker
            .ack_effect(&infrastructure_consumer, delivery.delivery_id)
            .await
            .unwrap();
    };
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
    let delivery = loop {
        let delivery = timeout(
            Duration::from_secs(10),
            broker.receive_effect_result(&consumer),
        )
        .await
        .unwrap()
        .unwrap();
        if delivery.result.event_id == result.event_id {
            break delivery;
        }
        broker
            .ack_effect_result(&consumer, delivery.delivery_id)
            .await
            .unwrap();
    };
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

fn agent_command(replica_id: Uuid, directive_id: Uuid) -> AgentCommand {
    AgentCommand {
        directive_id,
        replica_id,
        target: ActionTarget::Replica { replica_id },
        kind: AgentDirectiveKind::Diagnostics,
        issued_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::minutes(5),
    }
}
