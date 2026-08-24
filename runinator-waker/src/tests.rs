use clap::Parser;

use crate::config::Config;

#[test]
fn config_parser_uses_local_development_defaults() {
    let config =
        crate::config::normalize_config(Config::try_parse_from(["runinator-waker"]).unwrap());

    assert_eq!(config.waker_consumer_group, "runinator-waker");
    assert!(config.waker_id.starts_with("waker-"));
    assert_eq!(config.max_wake_sleep_seconds, 20);
    assert_eq!(config.broker_backend, "tcp");
    assert_eq!(config.broker_endpoint, "127.0.0.1:7070");
    assert_eq!(config.broker_mode, "direct");
    assert_eq!(config.broker_wake_topic, "runinator.wake");
    assert_eq!(config.broker_ingress_topic, "runinator.ingress");
    assert_eq!(config.broker_heartbeat_seconds, 10);
}

#[test]
fn config_parser_accepts_waker_and_broker_overrides() {
    let config = Config::try_parse_from([
        "runinator-waker",
        "--waker-consumer-group",
        "wake-workers",
        "--waker-id",
        "relay-1",
        "--max-wake-sleep-seconds",
        "5",
        "--broker-backend",
        "http",
        "--broker-endpoint",
        "127.0.0.1:9090",
        "--broker-client-id",
        "relay-1",
        "--broker-mode",
        "relay",
        "--service-url",
        "https://runinator.example.test/",
        "--api-key",
        "relay-key",
        "--broker-heartbeat-seconds",
        "5",
    ])
    .unwrap();

    assert_eq!(config.waker_consumer_group, "wake-workers");
    assert_eq!(config.waker_id, "relay-1");
    assert_eq!(config.max_wake_sleep_seconds, 5);
    assert_eq!(config.broker_backend, "http");
    assert_eq!(config.broker_endpoint, "127.0.0.1:9090");
    assert_eq!(config.broker_client_id, "relay-1");
    assert_eq!(config.broker_mode, "relay");
    assert_eq!(
        config.service_url.as_deref(),
        Some("https://runinator.example.test/")
    );
    assert_eq!(config.api_key.as_deref(), Some("relay-key"));
    assert_eq!(config.broker_heartbeat_seconds, 5);
}

#[test]
fn config_parser_rejects_control_plane_options() {
    for option in ["--api-base-url"] {
        assert!(
            Config::try_parse_from(["runinator-waker", option, "unused"]).is_err(),
            "{option} must not be accepted by the broker-only waker"
        );
    }
}

/// a terminal effect result carried by a wake, as the infrastructure effect host builds it.
fn effect_result(due_at: chrono::DateTime<chrono::Utc>) -> runinator_broker::EffectResult {
    runinator_broker::EffectResult {
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        event_id: uuid::Uuid::now_v7(),
        effect_id: uuid::Uuid::now_v7(),
        workflow_run_id: uuid::Uuid::now_v7(),
        continuation_id: uuid::Uuid::now_v7(),
        attempt: 0,
        kind: runinator_broker::EffectResultKind::Status {
            status: runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
            output: None,
            message: None,
        },
        timestamp: due_at,
        trace_id: uuid::Uuid::now_v7(),
        notification_delivery_id: None,
    }
}

#[tokio::test]
async fn due_wake_is_not_blocked_by_a_not_yet_due_wake() {
    use runinator_broker::{Broker, WakeCommand, WakeMessage, WsIngressCommand};
    use std::sync::Arc;

    let broker: Arc<dyn Broker> = Arc::new(runinator_broker::in_memory::InMemoryBroker::new());
    let now = chrono::Utc::now();

    // a far-future wake delivered first, then a due wake queued behind it.
    let future_due = now + chrono::Duration::seconds(60);
    let due_due = now - chrono::Duration::seconds(1);
    let future = WakeCommand::new(future_due, effect_result(future_due), uuid::Uuid::now_v7());
    let due = WakeCommand::new(due_due, effect_result(due_due), uuid::Uuid::now_v7());
    let due_effect_id = due.effect_id();
    for command in [future, due] {
        broker
            .publish_wake(WakeMessage {
                command,
                dedupe_key: None,
                enqueued_at: now,
            })
            .await
            .unwrap();
    }

    let config = Config::try_parse_from(["runinator-waker"]).unwrap();
    let notify = Arc::new(tokio::sync::Notify::new());
    let loop_broker = Arc::clone(&broker);
    let loop_notify = Arc::clone(&notify);
    let handle =
        tokio::spawn(async move { crate::waker_loop(loop_broker, loop_notify, &config).await });

    // the due wake's settle must arrive while the future wake is still sleeping toward its due
    // time; a serial waker would sit in that sleep and time this out.
    let delivery = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        broker.receive_ingress("test"),
    )
    .await
    .expect("due wake should be settled while the future wake sleeps")
    .unwrap();
    match &delivery.command {
        WsIngressCommand::SettleEffect { result, .. } => {
            assert_eq!(result.effect_id, due_effect_id)
        }
        other => panic!("expected a settle, got {other:?}"),
    }
    broker
        .ack_ingress("test", delivery.delivery_id)
        .await
        .unwrap();

    notify.notify_waiters();
    tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("waker loop should stop after shutdown")
        .unwrap();
}

#[tokio::test]
async fn a_relayed_settle_carries_the_armed_result_verbatim() {
    use runinator_broker::{Broker, WakeCommand, WakeMessage, WsIngressCommand};
    use std::sync::Arc;

    let broker: Arc<dyn Broker> = Arc::new(runinator_broker::in_memory::InMemoryBroker::new());
    let due_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    let result = effect_result(due_at);
    broker
        .publish_wake(WakeMessage {
            command: WakeCommand::new(due_at, result.clone(), uuid::Uuid::now_v7()),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let config = Config::try_parse_from(["runinator-waker"]).unwrap();
    let notify = Arc::new(tokio::sync::Notify::new());
    let loop_broker = Arc::clone(&broker);
    let loop_notify = Arc::clone(&notify);
    let handle =
        tokio::spawn(async move { crate::waker_loop(loop_broker, loop_notify, &config).await });

    let delivery = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        broker.receive_ingress("test"),
    )
    .await
    .expect("a due wake should be relayed")
    .unwrap();
    // the waker is a relay, not a decision point: it must not restamp, rebuild, or reinterpret the
    // result the effect host armed, because that result is what settles the effect.
    match &delivery.command {
        WsIngressCommand::SettleEffect {
            result: relayed, ..
        } => {
            assert_eq!(relayed.event_id, result.event_id);
            assert_eq!(relayed.effect_id, result.effect_id);
            assert_eq!(relayed.attempt, result.attempt);
            assert_eq!(relayed.timestamp, result.timestamp);
        }
        other => panic!("expected a settle, got {other:?}"),
    }

    notify.notify_waiters();
    tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("waker loop should stop after shutdown")
        .unwrap();
}

#[tokio::test]
async fn spawn_liveness_is_disabled_for_a_blank_path() {
    let mut config = Config::try_parse_from(["runinator-waker"]).unwrap();
    config.liveness_file = String::new();
    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    assert!(crate::spawn_liveness(&config, shutdown).is_none());
}

#[tokio::test]
async fn spawn_liveness_writes_the_configured_file() {
    let mut path = std::env::temp_dir();
    path.push(format!("runinator-waker-liveness-{}", uuid::Uuid::new_v4()));
    let mut config = Config::try_parse_from(["runinator-waker"]).unwrap();
    config.liveness_file = path.to_string_lossy().to_string();

    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    let handle =
        crate::spawn_liveness(&config, shutdown.clone()).expect("a path should spawn a task");

    for _ in 0..50 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(path.exists(), "waker should touch its liveness file");

    shutdown.notify_waiters();
    tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("liveness task should stop after shutdown")
        .expect("liveness task should not panic");
    let _ = std::fs::remove_file(&path);
}
