use std::sync::Arc;

use crate::{
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    errors::BrokerError,
    Broker,
};

// construct a kafka-backed broker, or fail when the kafka feature is disabled.
#[cfg(feature = "kafka")]
pub fn build_kafka_broker(config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, BrokerError> {
    Ok(Arc::new(crate::adapters::kafka::KafkaBroker::new(config)?))
}

#[cfg(not(feature = "kafka"))]
pub fn build_kafka_broker(_config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, BrokerError> {
    Err(BrokerError::FeatureDisabled("kafka"))
}

// construct a rabbitmq-backed broker, or fail when the rabbitmq feature is disabled.
#[cfg(feature = "rabbitmq")]
pub async fn build_rabbitmq_broker(
    config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, BrokerError> {
    Ok(Arc::new(
        crate::adapters::rabbitmq::RabbitMqBroker::connect(config).await?,
    ))
}

#[cfg(not(feature = "rabbitmq"))]
pub async fn build_rabbitmq_broker(
    _config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, BrokerError> {
    Err(BrokerError::FeatureDisabled("rabbitmq"))
}
