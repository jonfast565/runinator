use super::*;

#[test]
fn rabbitmq_config_defaults_queues_and_client_id() {
    let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f");

    assert_eq!(config.uri, "amqp://127.0.0.1:5672/%2f");
    assert_eq!(config.control_queue, DEFAULT_CONTROL_QUEUE);
    assert_eq!(config.effect_queue, DEFAULT_EFFECT_QUEUE);
    assert_eq!(
        config.infrastructure_effect_queue,
        DEFAULT_INFRASTRUCTURE_EFFECT_QUEUE
    );
    assert_eq!(config.effect_result_queue, DEFAULT_EFFECT_RESULT_QUEUE);
    assert_eq!(config.client_id, DEFAULT_CLIENT_ID);
}

#[test]
fn rabbitmq_config_accepts_queue_and_client_overrides() {
    let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f")
        .with_control_queue("c")
        .with_effect_queues("e", "i", "er")
        .with_client_id("test-client");

    assert_eq!(config.control_queue, "c");
    assert_eq!(config.effect_queue, "e");
    assert_eq!(config.infrastructure_effect_queue, "i");
    assert_eq!(config.effect_result_queue, "er");
    assert_eq!(config.client_id, "test-client");
}

#[test]
fn rabbitmq_config_requires_all_effect_queues() {
    let config =
        RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f").with_effect_queues("e", "i", "er");
    assert!(config.has_workflow_effect_queues());

    let config = config.with_effect_queues("e", "i", " ");
    assert!(!config.has_workflow_effect_queues());
}
