use super::*;

#[test]
fn disable_gossip_skips_advertiser_startup() {
    assert!(!should_spawn_gossip_advertiser(true));
    assert!(should_spawn_gossip_advertiser(false));
}

#[tokio::test]
async fn build_broker_rejects_kafka_without_result_topic() {
    let err = match build_broker(
        "kafka",
        "localhost:9092",
        KafkaBrokerConfig::new("localhost:9092").with_topics("actions", "control", " "),
        RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f"),
    )
    .await
    {
        Ok(_) => panic!("expected kafka result channel startup guard to fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("Broker backend 'kafka'"));
    assert!(err.to_string().contains("non-empty workflow result topic"));
}

#[tokio::test]
async fn build_broker_rejects_rabbitmq_without_result_queue() {
    let err = match build_broker(
        "rabbitmq",
        "amqp://127.0.0.1:5672/%2f",
        KafkaBrokerConfig::new("localhost:9092"),
        RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f")
            .with_queues("actions", "control", ""),
    )
    .await
    {
        Ok(_) => panic!("expected rabbitmq result channel startup guard to fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("Broker backend 'rabbitmq'"));
    assert!(err.to_string().contains("non-empty workflow result queue"));
}
