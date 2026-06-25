use clap::Parser;

use crate::config::Config;

#[test]
fn config_parser_uses_local_development_defaults() {
    let config = Config::try_parse_from(["runinator-waker"]).unwrap();

    assert_eq!(config.waker_id, "");
    assert_eq!(config.waker_consumer_group, "runinator-waker");
    assert_eq!(config.max_wake_sleep_seconds, 20);
    assert_eq!(config.broker_backend, "tcp");
    assert_eq!(config.broker_endpoint, "127.0.0.1:7070");
    assert_eq!(config.broker_wake_topic, "runinator.wake");
    assert_eq!(config.broker_ingress_topic, "runinator.ingress");
}

#[test]
fn config_parser_accepts_waker_and_broker_overrides() {
    let config = Config::try_parse_from([
        "runinator-waker",
        "--waker-id",
        "waker-a",
        "--waker-consumer-group",
        "wake-workers",
        "--max-wake-sleep-seconds",
        "5",
        "--broker-backend",
        "http",
        "--broker-endpoint",
        "127.0.0.1:9090",
        "--broker-client-id",
        "relay-1",
    ])
    .unwrap();

    assert_eq!(config.waker_id, "waker-a");
    assert_eq!(config.waker_consumer_group, "wake-workers");
    assert_eq!(config.max_wake_sleep_seconds, 5);
    assert_eq!(config.broker_backend, "http");
    assert_eq!(config.broker_endpoint, "127.0.0.1:9090");
    assert_eq!(config.broker_client_id, "relay-1");
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
