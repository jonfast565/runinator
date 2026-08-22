use super::*;

#[test]
fn kafka_config_defaults_topics_and_client_id() {
    let config = KafkaBrokerConfig::new("localhost:9092");

    assert_eq!(config.bootstrap_servers, "localhost:9092");
    assert_eq!(config.control_topic, DEFAULT_CONTROL_TOPIC);
    assert_eq!(config.effect_topic, DEFAULT_EFFECT_TOPIC);
    assert_eq!(
        config.infrastructure_effect_topic,
        DEFAULT_INFRASTRUCTURE_EFFECT_TOPIC
    );
    assert_eq!(config.effect_result_topic, DEFAULT_EFFECT_RESULT_TOPIC);
    assert_eq!(config.client_id, DEFAULT_CLIENT_ID);
}

#[test]
fn kafka_config_accepts_topic_and_client_overrides() {
    let config = KafkaBrokerConfig::new("localhost:9092")
        .with_control_topic("c")
        .with_effect_topics("e", "i", "er")
        .with_client_id("test-client");

    assert_eq!(config.control_topic, "c");
    assert_eq!(config.effect_topic, "e");
    assert_eq!(config.infrastructure_effect_topic, "i");
    assert_eq!(config.effect_result_topic, "er");
    assert_eq!(config.client_id, "test-client");
}

#[test]
fn kafka_config_requires_all_effect_topics() {
    let config = KafkaBrokerConfig::new("localhost:9092").with_effect_topics("e", "i", "er");
    assert!(config.has_workflow_effect_topics());

    let config = config.with_effect_topics("e", " ", "er");
    assert!(!config.has_workflow_effect_topics());
}
