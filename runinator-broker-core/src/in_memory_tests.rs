use chrono::Utc;
use runinator_comm::ActionCommand;
use std::sync::Arc;

use runinator_models::json;
use runinator_models::workflows::WorkflowAction;

use crate::{instrument, Broker, BrokerMessage, EffectMessage, EffectResultMessage, ResultMessage};

use super::*;

#[test]
fn in_memory_broker_supports_workflow_result_channels() {
    assert!(InMemoryBroker::new().supports_workflow_result_channels());
}

#[tokio::test]
async fn in_memory_broker_round_trips_vm_effects_without_action_identity() {
    // Production brokers are wrapped for telemetry. Keep the VM channels in this path so a newly
    // added Broker method cannot silently fall back to its `NotImplemented` default on the wrapper.
    let broker = instrument(Arc::new(InMemoryBroker::new()), "in-memory");
    let command = runinator_comm::EffectCommand {
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt: 0,
        request: runinator_models::workflow_vm::WorkflowEffectRequest::Timer { due_at: 42 },
        executor: runinator_comm::EffectExecutor::Infrastructure,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: "timer:42".into(),
        notification_delivery_id: None,
    };
    broker
        .publish_effect(EffectMessage {
            command: command.clone(),
            dedupe_key: None,
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();
    let delivery = broker
        .receive_infrastructure_effect("engine-infrastructure")
        .await
        .unwrap();
    assert_eq!(delivery.command.effect_id, command.effect_id);
    broker
        .ack_effect("engine-infrastructure", delivery.delivery_id)
        .await
        .unwrap();

    let result = runinator_comm::EffectResult::status(
        &command,
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
    let delivery = broker.receive_effect_result("ws").await.unwrap();
    assert_eq!(delivery.result.effect_id, result.effect_id);
    broker
        .ack_effect_result("ws", delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn agent_directives_route_only_to_the_target_replica() {
    let broker = InMemoryBroker::new();
    let target = Uuid::now_v7();
    broker
        .publish_agent(runinator_comm::AgentCommand {
            directive_id: Uuid::now_v7(),
            replica_id: target,
            target: runinator_comm::ActionTarget::Replica { replica_id: target },
            kind: runinator_comm::AgentDirectiveKind::Diagnostics,
            issued_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        })
        .await
        .unwrap();

    let profile = runinator_comm::ConsumerProfile::shared(target.to_string())
        .with_replica_id(target)
        .exclusive();
    let delivery = broker.receive_agent_for(&profile).await.unwrap();
    assert_eq!(delivery.command.replica_id, target);
    broker
        .ack_agent(&profile.id, delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn in_memory_broker_redelivers_expired_action_delivery() {
    let broker = InMemoryBroker::with_lease_duration(Duration::from_millis(10));
    broker
        .publish(BrokerMessage {
            command: action_command(),
            dedupe_key: Some("lease-action".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let first = broker.receive("consumer-a").await.unwrap();
    tokio::time::sleep(Duration::from_millis(15)).await;
    let second = broker.receive("consumer-b").await.unwrap();

    assert_ne!(first.delivery_id, second.delivery_id);
    assert_eq!(first.command.command_id, second.command.command_id);
    assert!(broker.ack("consumer-a", first.delivery_id).await.is_err());
    broker.ack("consumer-b", second.delivery_id).await.unwrap();
}

#[tokio::test]
async fn in_memory_broker_redelivers_expired_result_delivery() {
    let broker = InMemoryBroker::with_lease_duration(Duration::from_millis(10));
    let command = action_command();
    let event = runinator_comm::WorkflowResultEvent::status(
        &command,
        runinator_models::workflows::WorkflowStatus::Succeeded,
        None,
        None,
    );
    broker
        .publish_result(ResultMessage {
            event,
            dedupe_key: Some("lease-result".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let first = broker.receive_result("consumer-a").await.unwrap();
    tokio::time::sleep(Duration::from_millis(15)).await;
    let second = broker.receive_result("consumer-b").await.unwrap();

    assert_ne!(first.delivery_id, second.delivery_id);
    assert_eq!(first.event.event_id, second.event.event_id);
    assert!(broker
        .ack_result("consumer-a", first.delivery_id)
        .await
        .is_err());
    broker
        .ack_result("consumer-b", second.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn in_memory_broker_redelivers_expired_wake_delivery() {
    let broker = InMemoryBroker::with_lease_duration(Duration::from_millis(10));
    let command = runinator_comm::WakeCommand::new(
        Uuid::now_v7(),
        Uuid::now_v7(),
        "node-a".into(),
        Utc::now(),
        Uuid::new_v4(),
        Uuid::now_v7(),
    );
    broker
        .publish_wake(crate::WakeMessage {
            command,
            dedupe_key: Some("lease-wake".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let first = broker.receive_wake("consumer-a").await.unwrap();
    tokio::time::sleep(Duration::from_millis(15)).await;
    let second = broker.receive_wake("consumer-b").await.unwrap();

    assert_ne!(first.delivery_id, second.delivery_id);
    assert_eq!(first.command.ready_node_id, second.command.ready_node_id);
    assert!(broker
        .ack_wake("consumer-a", first.delivery_id)
        .await
        .is_err());
    broker
        .ack_wake("consumer-b", second.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn in_memory_broker_round_trips_ingress_delivery() {
    let broker = InMemoryBroker::new();
    let command = runinator_comm::WsIngressCommand::drive(
        Uuid::now_v7(),
        Uuid::now_v7(),
        "node-a".into(),
        Uuid::now_v7(),
    );
    broker
        .publish_ingress(crate::IngressMessage {
            command,
            dedupe_key: None,
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let delivery = broker.receive_ingress("ws").await.unwrap();
    assert!(matches!(
        delivery.command,
        runinator_comm::WsIngressCommand::Drive { .. }
    ));
    broker
        .ack_ingress("ws", delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn in_memory_broker_fans_out_events_to_every_subscriber() {
    use crate::EventMessage;
    use runinator_comm::UiEvent;

    let broker = InMemoryBroker::new();
    // both subscribers must register before publishing so each gets the event.
    let _ = broker.event_receiver("ws-a");
    let _ = broker.event_receiver("ws-b");

    broker
        .publish_event(EventMessage::new(UiEvent::global(
            runinator_comm::UiEventKind::WorkflowsChanged,
        )))
        .await
        .unwrap();

    let a = broker.receive_event("ws-a").await.unwrap();
    let b = broker.receive_event("ws-b").await.unwrap();
    assert!(matches!(
        a.event.kind,
        runinator_comm::UiEventKind::WorkflowsChanged
    ));
    assert!(matches!(
        b.event.kind,
        runinator_comm::UiEventKind::WorkflowsChanged
    ));
}

#[tokio::test]
async fn receive_control_for_routes_targeted_controls_to_the_matching_replica() {
    use runinator_comm::{ConsumerProfile, ControlCommand, ControlKind};

    let broker = InMemoryBroker::new();
    let holder = Uuid::now_v7();
    let bystander = Uuid::now_v7();
    let run_id = Uuid::now_v7();

    // a cancel routed to the executor-holding replica, queued behind nothing special.
    let targeted = ControlCommand::for_node_run(run_id, Uuid::now_v7(), ControlKind::Cancel)
        .targeting_replica(holder);
    broker.publish_control(targeted).await.unwrap();

    // a worker that is not the holder must never receive it, even when polling first; the
    // 50ms timeout bounds the test rather than proving absence forever.
    let bystander_profile = ConsumerProfile::shared("worker-b").with_replica_id(bystander);
    let unmatched = tokio::time::timeout(
        Duration::from_millis(50),
        broker.receive_control_for(&bystander_profile),
    )
    .await;
    assert!(unmatched.is_err(), "targeted control leaked to a bystander");

    // the holder receives it.
    let holder_profile = ConsumerProfile::shared("worker-a").with_replica_id(holder);
    let delivery = broker.receive_control_for(&holder_profile).await.unwrap();
    assert_eq!(delivery.command.workflow_run_id, run_id);
    broker
        .ack_control("worker-a", delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn receive_control_for_hands_untargeted_controls_to_any_non_exclusive_profile() {
    use runinator_comm::{ConsumerProfile, ControlCommand, ControlKind};

    let broker = InMemoryBroker::new();
    let run_id = Uuid::now_v7();
    broker
        .publish_control(ControlCommand::new(run_id, ControlKind::Cancel))
        .await
        .unwrap();

    // the worker's control profile is never exclusive, so a run-wide `Any` control matches a
    // replica-bound profile too (a desktop worker must still see run-wide cancels).
    let profile = ConsumerProfile::shared("desktop").with_replica_id(Uuid::now_v7());
    let delivery = broker.receive_control_for(&profile).await.unwrap();
    assert_eq!(delivery.command.workflow_run_id, run_id);
}

#[tokio::test]
async fn stale_queued_controls_are_dropped_not_delivered() {
    use runinator_comm::{ControlCommand, ControlKind};

    let mut state = BrokerState::default();
    let delivery: ControlDelivery = ControlCommand::new(Uuid::now_v7(), ControlKind::Cancel).into();
    state.control_queue.push_back(delivery);

    // fresh controls survive the sweep; one past the ttl is dropped.
    state.drop_stale_control(chrono::Utc::now());
    assert_eq!(state.control_queue.len(), 1);
    let past_ttl =
        chrono::Utc::now() + chrono::Duration::seconds(crate::STALE_CONTROL_TTL_SECONDS + 1);
    state.drop_stale_control(past_ttl);
    assert!(state.control_queue.is_empty());
}

#[tokio::test]
async fn nack_control_requeues_for_the_matching_consumer() {
    use runinator_comm::{ConsumerProfile, ControlCommand, ControlKind};

    let broker = InMemoryBroker::new();
    let run_id = Uuid::now_v7();
    broker
        .publish_control(ControlCommand::new(run_id, ControlKind::Pause))
        .await
        .unwrap();

    // the legacy untargeted path takes it; a nack must return it for redelivery.
    let first = broker.receive_control("worker-a").await.unwrap();
    broker
        .nack_control("worker-a", first.delivery_id)
        .await
        .unwrap();
    let second = broker
        .receive_control_for(&ConsumerProfile::shared("worker-b"))
        .await
        .unwrap();
    assert_eq!(second.command.workflow_run_id, run_id);
    assert_ne!(first.delivery_id, second.delivery_id);
}

#[tokio::test]
async fn receive_for_routes_targeted_actions_to_the_matching_consumer() {
    use runinator_comm::{ActionTarget, ConsumerProfile};

    let broker = InMemoryBroker::new();
    let replica = Uuid::now_v7();

    // a replica-targeted action and a general-pool (Any) action share the queue.
    let mut targeted = action_command();
    targeted.target = ActionTarget::Replica {
        replica_id: replica,
    };
    let any = action_command();
    broker
        .publish(BrokerMessage {
            command: targeted.clone(),
            dedupe_key: Some("targeted".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();
    broker
        .publish(BrokerMessage {
            command: any.clone(),
            dedupe_key: Some("any".into()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    // an exclusive consumer bound to the replica only sees the targeted action, even though it
    // sits ahead of nothing special; it must never receive the Any action.
    let desktop = ConsumerProfile::shared("desktop")
        .with_replica_id(replica)
        .exclusive();
    let delivery = broker.receive_for(&desktop).await.unwrap();
    assert_eq!(delivery.command.command_id, targeted.command_id);
    broker.ack("desktop", delivery.delivery_id).await.unwrap();

    // a general-pool consumer picks up the remaining Any action.
    let server = ConsumerProfile::shared("server");
    let delivery = broker.receive_for(&server).await.unwrap();
    assert_eq!(delivery.command.command_id, any.command_id);
}

fn action_command() -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: Uuid::now_v7(),
        workflow_node_run_id: Uuid::now_v7(),
        node_id: "node-a".into(),
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
        parameters: json!({}),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        notification_delivery_id: None,
        invocation_call_id: None,
        task_run_id: None,
        idempotency_key: None,
    }
}
