use std::sync::Arc;

use runinator_broker::{
    Broker, BrokerError,
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    http::client::HttpBroker,
    in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
};
use runinator_models::errors::{RuntimeError, SendableError};

use crate::config;

pub async fn build_broker(config: &config::Config) -> Result<Arc<dyn Broker>, SendableError> {
    runinator_broker::ensure_named_workflow_result_channel(
        &config.broker_backend,
        &config.broker_result_topic,
    )
    .map_err(|err| broker_error("workflow_results", err))?;

    let broker: Arc<dyn Broker> = match config.broker_backend.as_str() {
        "http" => {
            let url = reqwest::Url::parse(&config.broker_endpoint)
                .map_err(|err| crate::errors::BROKER_INVALID_ENDPOINT.error(err))?;

            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| crate::errors::BROKER_CLIENT.error(err))?;

            Arc::new(HttpBroker::new(url, client))
        }
        "in-memory" => Arc::new(InMemoryBroker::new()),
        "tcp" => Arc::new(TcpBroker::new(config.broker_endpoint.clone())),
        "kafka" => runinator_broker::build_kafka_broker(
            KafkaBrokerConfig::new(config.broker_endpoint.clone())
                .with_topics(
                    config.broker_action_topic.clone(),
                    config.broker_control_topic.clone(),
                    config.broker_result_topic.clone(),
                )
                .with_client_id(config.broker_client_id.clone()),
        )
        .map_err(|err| crate::errors::BROKER_KAFKA.error(err))?,
        "rabbitmq" => runinator_broker::build_rabbitmq_broker(
            RabbitMqBrokerConfig::new(config.broker_endpoint.clone())
                .with_queues(
                    config.broker_action_topic.clone(),
                    config.broker_control_topic.clone(),
                    config.broker_result_topic.clone(),
                )
                .with_client_id(config.broker_client_id.clone()),
        )
        .await
        .map_err(|err| crate::errors::BROKER_RABBITMQ.error(err))?,
        other => {
            return Err(crate::errors::BROKER_UNKNOWN_BACKEND.error(format!("'{other}'")));
        }
    };

    runinator_broker::ensure_workflow_result_channels_supported(
        &config.broker_backend,
        broker.as_ref(),
    )
    .map_err(|err| broker_error("workflow_results", err))?;

    Ok(broker)
}

pub(crate) fn broker_error(context: &'static str, err: BrokerError) -> SendableError {
    // keep the per-context dotted key for back-compat while rendering the numbered code.
    let descriptor = crate::errors::BROKER_OPERATION;
    Box::new(RuntimeError::new(
        format!("worker.broker.{context}"),
        format!(
            "{} - {}: {context}: {err}",
            descriptor.code, descriptor.summary
        ),
    ))
}
