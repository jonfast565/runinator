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

pub(crate) async fn build_broker(
    config: &config::Config,
) -> Result<Arc<dyn Broker>, SendableError> {
    runinator_broker::ensure_named_workflow_result_channel(
        &config.broker_backend,
        &config.broker_result_topic,
    )
    .map_err(|err| broker_error("workflow_results", err))?;

    let broker: Arc<dyn Broker> = match config.broker_backend.as_str() {
        "http" => {
            let url = reqwest::Url::parse(&config.broker_endpoint).map_err(|err| {
                Box::new(RuntimeError::new(
                    "worker.broker.invalid_endpoint".into(),
                    err.to_string(),
                )) as SendableError
            })?;

            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| -> SendableError {
                    Box::new(RuntimeError::new(
                        "worker.broker.client".into(),
                        err.to_string(),
                    ))
                })?;

            Arc::new(HttpBroker::new(url, client))
        }
        "in-memory" => Arc::new(InMemoryBroker::new()),
        "tcp" => Arc::new(TcpBroker::new(config.broker_endpoint.clone())),
        "kafka" => build_kafka_broker(
            KafkaBrokerConfig::new(config.broker_endpoint.clone())
                .with_topics(
                    config.broker_action_topic.clone(),
                    config.broker_control_topic.clone(),
                    config.broker_result_topic.clone(),
                )
                .with_client_id(config.broker_client_id.clone()),
        )?,
        "rabbitmq" => {
            build_rabbitmq_broker(
                RabbitMqBrokerConfig::new(config.broker_endpoint.clone())
                    .with_queues(
                        config.broker_action_topic.clone(),
                        config.broker_control_topic.clone(),
                        config.broker_result_topic.clone(),
                    )
                    .with_client_id(config.broker_client_id.clone()),
            )
            .await?
        }
        other => {
            return Err(Box::new(RuntimeError::new(
                "worker.broker.unknown_backend".into(),
                format!("Unknown broker backend '{other}'"),
            )));
        }
    };

    runinator_broker::ensure_workflow_result_channels_supported(
        &config.broker_backend,
        broker.as_ref(),
    )
    .map_err(|err| broker_error("workflow_results", err))?;

    Ok(broker)
}

#[cfg(feature = "kafka")]
fn build_kafka_broker(config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::kafka::KafkaBroker::new(config).map_err(
        |err| -> SendableError {
            Box::new(RuntimeError::new(
                "worker.broker.kafka".into(),
                err.to_string(),
            ))
        },
    )?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "kafka"))]
fn build_kafka_broker(_config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    Err(Box::new(RuntimeError::new(
        "worker.broker.kafka_feature_disabled".into(),
        "Broker backend 'kafka' requires building runinator-worker with --features kafka".into(),
    )))
}

#[cfg(feature = "rabbitmq")]
async fn build_rabbitmq_broker(
    config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::rabbitmq::RabbitMqBroker::connect(config)
        .await
        .map_err(|err| -> SendableError {
            Box::new(RuntimeError::new(
                "worker.broker.rabbitmq".into(),
                err.to_string(),
            ))
        })?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "rabbitmq"))]
async fn build_rabbitmq_broker(
    _config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    Err(Box::new(RuntimeError::new(
        "worker.broker.rabbitmq_feature_disabled".into(),
        "Broker backend 'rabbitmq' requires building runinator-worker with --features rabbitmq"
            .into(),
    )))
}

pub(crate) fn broker_error(context: &'static str, err: BrokerError) -> SendableError {
    Box::new(RuntimeError::new(
        format!("worker.broker.{context}"),
        err.to_string(),
    ))
}
