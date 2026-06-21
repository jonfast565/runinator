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
