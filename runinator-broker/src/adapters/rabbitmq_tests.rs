use super::*;

#[test]
fn rabbitmq_config_defaults_queues_and_client_id() {
    let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f");

    assert_eq!(config.uri, "amqp://127.0.0.1:5672/%2f");
    assert_eq!(config.action_queue, DEFAULT_ACTION_QUEUE);
    assert_eq!(config.control_queue, DEFAULT_CONTROL_QUEUE);
    assert_eq!(config.result_queue, DEFAULT_RESULT_QUEUE);
    assert_eq!(config.client_id, DEFAULT_CLIENT_ID);
}

#[test]
fn rabbitmq_config_defaults_targeted_action_queue() {
    let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f");

    assert_eq!(
        config.targeted_action_queue,
        format!("{DEFAULT_ACTION_QUEUE}.targeted")
    );

    let config = config.with_targeted_action_queue("custom.targeted");
    assert_eq!(config.targeted_action_queue, "custom.targeted");
}

#[test]
fn rabbitmq_config_accepts_queue_and_client_overrides() {
    let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f")
        .with_queues("a", "c", "r")
        .with_client_id("test-client");

    assert_eq!(config.action_queue, "a");
    assert_eq!(config.control_queue, "c");
    assert_eq!(config.result_queue, "r");
    assert_eq!(config.client_id, "test-client");
}

#[test]
fn rabbitmq_config_detects_missing_result_queue() {
    let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f").with_queues("a", "c", " ");

    assert!(!config.has_workflow_result_queue());
}
