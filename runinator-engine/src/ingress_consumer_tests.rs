//! covers the ingress consumer: relaying a due timer wake onto the effect-result channel, and
//! recording an agent's reply to a durable directive.

use super::*;

use runinator_broker_core::{IngressMessage, in_memory::InMemoryBroker};
use runinator_comm::{
    AgentDirectiveKind, AgentDirectiveResult, AgentDirectiveStatus, EffectResultKind,
    ReplicaAvailability, WsIngressCommand,
};
use runinator_models::replicas::{ReplicaKind, ReplicaRegistrationRequest, ReplicaStatus};
use runinator_models::workflow_vm::WorkflowEffectStatus;
use runinator_store::prelude::*;
use uuid::Uuid;

/// a sqlite store on a temporary file, returned with the path so the caller can remove it.
async fn store() -> (
    Arc<runinator_database::sqlite::SqliteDb>,
    std::path::PathBuf,
) {
    let path = std::env::temp_dir().join(format!("runinator-ingress-{}.db", Uuid::now_v7()));
    let db = Arc::new(
        runinator_database::sqlite::SqliteDb::new(path.to_str().expect("temporary database path"))
            .await
            .unwrap(),
    );
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (db, path)
}

fn effect_result(effect_id: Uuid, attempt: u32) -> runinator_comm::EffectResult {
    runinator_comm::EffectResult {
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        event_id: Uuid::now_v7(),
        effect_id,
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt,
        kind: EffectResultKind::Status {
            status: WorkflowEffectStatus::Succeeded,
            output: None,
            message: None,
        },
        timestamp: chrono::Utc::now(),
        trace_id: Uuid::now_v7(),
        notification_delivery_id: None,
    }
}

#[tokio::test]
async fn a_due_timer_wake_is_relayed_onto_the_effect_result_channel() {
    let (db, path) = store().await;
    let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
    let shutdown = Arc::new(Notify::new());
    let consumer = tokio::spawn(run_ingress_consumer(db, broker.clone(), shutdown.clone()));

    let effect_id = Uuid::now_v7();
    let result = effect_result(effect_id, 3);
    broker
        .publish_ingress(IngressMessage {
            command: WsIngressCommand::settle_effect(result.clone(), Uuid::now_v7()),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    // the settle must arrive verbatim: the effect-result consumer is the only settle path, so the
    // relay must not rebuild or restamp what the infrastructure host already decided.
    let delivery =
        tokio::time::timeout(Duration::from_secs(5), broker.receive_effect_result("test"))
            .await
            .expect("the relayed settle should reach the effect-result channel")
            .unwrap();
    assert_eq!(delivery.result.effect_id, effect_id);
    assert_eq!(delivery.result.attempt, 3);
    assert_eq!(delivery.result.event_id, result.event_id);
    assert_eq!(delivery.result.timestamp, result.timestamp);

    shutdown.notify_waiters();
    consumer.await.unwrap();
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn an_agent_directive_reply_completes_its_durable_record() {
    let (db, path) = store().await;
    let replica = db
        .register_replica(
            runinator_models::replicas::ReplicaRegistrationRequest {
                replica_id: None,
                replica_type: runinator_models::replicas::ReplicaKind::Worker,
                instance_id: "ingress-agent".to_string(),
                runtime_id: "ingress-runtime".to_string(),
                display_name: None,
                host: None,
                port: None,
                base_path: None,
                version: None,
                attributes: runinator_models::json!({}),
            },
            None,
            &runinator_models::auth::AuthContext::disabled_platform_admin(),
        )
        .await
        .unwrap();
    let directive = db
        .enqueue_agent_directive(
            replica.replica_id,
            AgentDirectiveKind::Diagnostics,
            chrono::Utc::now() + chrono::Duration::minutes(5),
        )
        .await
        .unwrap();

    let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
    let shutdown = Arc::new(Notify::new());
    let consumer = tokio::spawn(run_ingress_consumer(
        db.clone(),
        broker.clone(),
        shutdown.clone(),
    ));

    broker
        .publish_ingress(IngressMessage {
            command: WsIngressCommand::AgentDirectiveResult {
                result: AgentDirectiveResult {
                    directive_id: directive.directive_id,
                    status: AgentDirectiveStatus::Completed,
                    payload: runinator_models::json!({ "ok": true }),
                    message: None,
                },
            },
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    // without a consumer this reply is dropped and the directive stays pending forever.
    let mut completed = false;
    for _ in 0..100 {
        let records = db
            .list_agent_directives(replica.replica_id, 10)
            .await
            .unwrap();
        if records
            .iter()
            .any(|record| record.state == runinator_comm::AgentDirectiveState::Completed)
        {
            completed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(completed, "the directive should be recorded as completed");

    shutdown.notify_waiters();
    consumer.await.unwrap();
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_reply_for_an_unknown_directive_is_acknowledged_rather_than_requeued() {
    let (db, path) = store().await;
    let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
    let shutdown = Arc::new(Notify::new());
    let consumer = tokio::spawn(run_ingress_consumer(db, broker.clone(), shutdown.clone()));

    broker
        .publish_ingress(IngressMessage {
            command: WsIngressCommand::AgentDirectiveResult {
                result: AgentDirectiveResult {
                    directive_id: Uuid::now_v7(),
                    status: AgentDirectiveStatus::Completed,
                    payload: runinator_models::json!({}),
                    message: None,
                },
            },
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    // a message that can never apply must not be requeued forever in front of the channel. once it
    // is acked, a later message is delivered; a nack loop would starve this one out.
    let effect_id = Uuid::now_v7();
    broker
        .publish_ingress(IngressMessage {
            command: WsIngressCommand::settle_effect(effect_result(effect_id, 0), Uuid::now_v7()),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let delivery =
        tokio::time::timeout(Duration::from_secs(5), broker.receive_effect_result("test"))
            .await
            .expect("the unknown directive must not block the message behind it")
            .unwrap();
    assert_eq!(delivery.result.effect_id, effect_id);

    shutdown.notify_waiters();
    consumer.await.unwrap();
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn broker_announced_replica_lifecycle_is_visible_and_retires_cleanly() {
    let (db, path) = store().await;
    let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
    let shutdown = Arc::new(Notify::new());
    let consumer = tokio::spawn(run_ingress_consumer(
        db.clone(),
        broker.clone(),
        shutdown.clone(),
    ));
    let replica_id = Uuid::now_v7();
    let runtime_id = replica_id.to_string();
    let registration = ReplicaRegistrationRequest {
        replica_id: Some(replica_id),
        replica_type: ReplicaKind::Waker,
        instance_id: "waker-test".to_string(),
        runtime_id: runtime_id.clone(),
        display_name: Some("waker-test".to_string()),
        host: None,
        port: None,
        base_path: None,
        version: None,
        attributes: runinator_models::json!({ "broker_backend": "in-memory" }),
    };
    broker
        .publish_ingress(IngressMessage {
            command: WsIngressCommand::ReplicaAvailability {
                availability: ReplicaAvailability::Available {
                    registration,
                    providers: Vec::new(),
                },
            },
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let mut registered = false;
    for _ in 0..100 {
        if matches!(db.fetch_replica(replica_id).await.unwrap(), Some(replica) if replica.status == ReplicaStatus::Live)
        {
            registered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        registered,
        "broker lifecycle must create a live waker record"
    );

    broker
        .publish_ingress(IngressMessage {
            command: WsIngressCommand::replica_offline(replica_id, runtime_id),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let mut offline = false;
    for _ in 0..100 {
        if matches!(db.fetch_replica(replica_id).await.unwrap(), Some(replica) if replica.status == ReplicaStatus::Offline)
        {
            offline = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        offline,
        "clean broker lifecycle shutdown must retire the replica"
    );

    shutdown.notify_waiters();
    consumer.await.unwrap();
    let _ = std::fs::remove_file(path);
}

/// The arming and the settling halves meet through the wake/ingress contract.
///
/// The relay in the middle is what `runinator-waker` does (sleep until `due_at`, publish the
/// carried result on ingress); the waker cannot be linked here, since it depends on the broker and
/// not on the engine. What this pins is that the two engine-side halves agree on the payload: an
/// effect armed by the infrastructure host is settled by the ingress consumer, with no in-process
/// task held open across the wait.
#[tokio::test]
async fn an_armed_timer_settles_through_the_wake_and_ingress_channels() {
    let (db, path) = store().await;
    let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
    let shutdown = Arc::new(Notify::new());

    let host = tokio::spawn(crate::run_infrastructure_effect_host(
        db.clone(),
        broker.clone(),
        shutdown.clone(),
    ));
    let consumer = tokio::spawn(run_ingress_consumer(db, broker.clone(), shutdown.clone()));

    // far enough out that a loaded machine cannot make it already-due before the host reads it,
    // which would settle it inline and arm no wake at all.
    let due_at = chrono::Utc::now() + chrono::Duration::seconds(30);
    let command = runinator_comm::EffectCommand {
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt: 0,
        request: runinator_models::workflow_vm::WorkflowEffectRequest::Timer {
            due_at: due_at.timestamp(),
        },
        executor: runinator_comm::EffectExecutor::Infrastructure,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: Uuid::now_v7().to_string(),
        notification_delivery_id: None,
    };
    broker
        .publish_effect(runinator_broker_core::EffectMessage {
            command: command.clone(),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let wake = tokio::time::timeout(Duration::from_secs(5), broker.receive_wake("relay"))
        .await
        .expect("the host should arm a wake")
        .unwrap();
    assert_eq!(wake.command.effect_id(), command.effect_id);

    // the relay's sleep is the waker's own contract and is tested there; what matters here is that
    // the result it carries is what settles the effect.
    broker
        .publish_ingress(IngressMessage {
            command: WsIngressCommand::settle_effect(
                wake.command.result.clone(),
                wake.command.trace_id,
            ),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    broker.ack_wake("relay", wake.delivery_id).await.unwrap();

    let delivery =
        tokio::time::timeout(Duration::from_secs(5), broker.receive_effect_result("test"))
            .await
            .expect("the relayed wake should settle the effect")
            .unwrap();
    assert_eq!(delivery.result.effect_id, command.effect_id);
    assert!(matches!(
        delivery.result.kind,
        EffectResultKind::Status {
            status: WorkflowEffectStatus::Succeeded,
            ..
        }
    ));

    shutdown.notify_waiters();
    host.await.unwrap();
    consumer.await.unwrap();
    let _ = std::fs::remove_file(path);
}
