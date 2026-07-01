use std::ffi::OsString;

use runinator_worker::Config;
use uuid::Uuid;

use crate::{
    REGISTER_BASE_BACKOFF, REGISTER_MAX_BACKOFF, provider_service_url_fallback, register_backoff,
    spawn_liveness,
};

#[test]
fn provider_service_url_uses_api_base_url_when_env_is_missing() {
    assert_eq!(
        provider_service_url_fallback(None, "http://127.0.0.1:8080/"),
        Some(OsString::from("http://127.0.0.1:8080/"))
    );
}

#[test]
fn provider_service_url_preserves_existing_env() {
    assert_eq!(
        provider_service_url_fallback(
            Some(OsString::from("http://127.0.0.1:9090/")),
            "http://127.0.0.1:8080/",
        ),
        None
    );
}

#[test]
fn provider_service_url_replaces_empty_env() {
    assert_eq!(
        provider_service_url_fallback(Some(OsString::from("  ")), "http://127.0.0.1:8080/"),
        Some(OsString::from("http://127.0.0.1:8080/"))
    );
}

#[test]
fn register_backoff_grows_then_caps() {
    assert_eq!(register_backoff(1), REGISTER_BASE_BACKOFF);
    assert_eq!(register_backoff(2), REGISTER_BASE_BACKOFF * 2);
    assert_eq!(register_backoff(3), REGISTER_BASE_BACKOFF * 4);
    // large attempts saturate at the cap instead of overflowing.
    assert_eq!(register_backoff(64), REGISTER_MAX_BACKOFF);
    assert_eq!(register_backoff(u32::MAX), REGISTER_MAX_BACKOFF);
}

#[tokio::test]
async fn spawn_liveness_is_disabled_without_a_path() {
    let config = test_config();
    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    assert!(spawn_liveness(&config, shutdown).is_none());
}

#[tokio::test]
async fn spawn_liveness_writes_the_configured_file() {
    let mut path = std::env::temp_dir();
    path.push(format!("runinator-worker-liveness-{}", Uuid::new_v4()));
    let mut config = test_config();
    config.liveness_file = path.to_string_lossy().to_string();

    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    let handle = spawn_liveness(&config, shutdown.clone()).expect("a path should spawn a task");

    for _ in 0..50 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(path.exists(), "worker should touch its liveness file");

    shutdown.notify_waiters();
    tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("liveness task should stop after shutdown")
        .expect("liveness task should not panic");
    let _ = std::fs::remove_file(&path);
}

fn test_config() -> Config {
    Config {
        dll_paths: Vec::new(),
        broker_backend: "in-memory".into(),
        broker_endpoint: "127.0.0.1:7070".into(),
        broker_action_topic: "runinator.actions".into(),
        broker_control_topic: "runinator.control".into(),
        broker_result_topic: "runinator.results".into(),
        broker_client_id: "test-worker".into(),
        broker_consumer_id: "test-consumer".into(),
        max_concurrent_actions: 1,
        shutdown_grace_seconds: 30,
        api_base_url: "http://127.0.0.1:8080/".into(),
        api_key: None,
        worker_id: Uuid::new_v4(),
        advertise_host: None,
        liveness_file: String::new(),
        labels: Default::default(),
    }
}
