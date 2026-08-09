use super::*;

#[test]
fn kafka_config_defaults_topics_and_client_id() {
    let config = KafkaBrokerConfig::new("localhost:9092");

    assert_eq!(config.bootstrap_servers, "localhost:9092");
    assert_eq!(config.action_topic, DEFAULT_ACTION_TOPIC);
    assert_eq!(config.control_topic, DEFAULT_CONTROL_TOPIC);
    assert_eq!(config.result_topic, DEFAULT_RESULT_TOPIC);
    assert_eq!(config.client_id, DEFAULT_CLIENT_ID);
}

#[test]
fn kafka_config_accepts_topic_and_client_overrides() {
    let config = KafkaBrokerConfig::new("localhost:9092")
        .with_topics("a", "c", "r")
        .with_client_id("test-client");

    assert_eq!(config.action_topic, "a");
    assert_eq!(config.control_topic, "c");
    assert_eq!(config.result_topic, "r");
    assert_eq!(config.client_id, "test-client");
}

#[test]
fn kafka_config_detects_missing_result_topic() {
    let config = KafkaBrokerConfig::new("localhost:9092").with_topics("a", "c", " ");

    assert!(!config.has_workflow_result_topic());
}
