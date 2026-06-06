use std::sync::Arc;

use log::{error, info};
use reqwest::Url;
use runinator_broker::{
    Broker,
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    http::client::HttpBroker,
    in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
};
use runinator_models::errors::SendableError;
use tokio::sync::Notify;

use runinator_utilities::startup;
use runinator_waker::{
    config::{Config, parse_config},
    waker_loop,
};

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Waker")?;

    info!("Parse waker config");
    let config = parse_config()?;
    info!("Waker ID: {}", config.waker_id);

    let broker = build_broker(&config).await?;
    let notify = Arc::new(Notify::new());

    let loop_notify = notify.clone();
    let loop_broker = broker.clone();
    let loop_config = config.clone();
    let handle = tokio::spawn(async move {
        waker_loop(loop_broker, loop_notify, &loop_config).await;
    });

    tokio::signal::ctrl_c()
        .await
        .map_err(|err| runinator_waker::errors::SIGNAL_CTRL_C.error(err))?;
    info!("Received shutdown signal. Shutting down...");
    notify.notify_waiters();
    if let Err(err) = handle.await {
        error!("Error while shutting down waker: {:?}", err);
    }

    info!("Waker shutdown complete.");
    Ok(())
}

async fn build_broker(config: &Config) -> Result<Arc<dyn Broker>, SendableError> {
    match config.broker_backend.as_str() {
        "http" => {
            let url = Url::parse(&config.broker_endpoint)
                .map_err(|err| runinator_waker::errors::BROKER_INVALID_ENDPOINT.error(err))?;
            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| runinator_waker::errors::BROKER_CLIENT.error(err))?;
            Ok(Arc::new(HttpBroker::new(url, client)))
        }
        "in-memory" => Ok(Arc::new(InMemoryBroker::new())),
        "tcp" => Ok(Arc::new(TcpBroker::new(config.broker_endpoint.clone()))),
        "kafka" => build_kafka_broker(
            KafkaBrokerConfig::new(config.broker_endpoint.clone())
                .with_topics(
                    config.broker_action_topic.clone(),
                    config.broker_control_topic.clone(),
                    config.broker_result_topic.clone(),
                )
                .with_orchestration_topics(
                    config.broker_wake_topic.clone(),
                    config.broker_ingress_topic.clone(),
                )
                .with_client_id(config.broker_client_id.clone()),
        ),
        "rabbitmq" => {
            build_rabbitmq_broker(
                RabbitMqBrokerConfig::new(config.broker_endpoint.clone())
                    .with_queues(
                        config.broker_action_topic.clone(),
                        config.broker_control_topic.clone(),
                        config.broker_result_topic.clone(),
                    )
                    .with_orchestration_queues(
                        config.broker_wake_topic.clone(),
                        config.broker_ingress_topic.clone(),
                    )
                    .with_client_id(config.broker_client_id.clone()),
            )
            .await
        }
        other => Err(runinator_waker::errors::BROKER_UNKNOWN_BACKEND.error(format!("'{other}'"))),
    }
}

#[cfg(feature = "kafka")]
fn build_kafka_broker(config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::kafka::KafkaBroker::new(config)
        .map_err(|err| runinator_waker::errors::BROKER_KAFKA.error(err))?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "kafka"))]
fn build_kafka_broker(_config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    Err(runinator_waker::errors::BROKER_KAFKA_FEATURE_DISABLED
        .error("build runinator-waker with --features kafka"))
}

#[cfg(feature = "rabbitmq")]
async fn build_rabbitmq_broker(
    config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::rabbitmq::RabbitMqBroker::connect(config)
        .await
        .map_err(|err| runinator_waker::errors::BROKER_RABBITMQ.error(err))?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "rabbitmq"))]
async fn build_rabbitmq_broker(
    _config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    Err(runinator_waker::errors::BROKER_RABBITMQ_FEATURE_DISABLED
        .error("build runinator-waker with --features rabbitmq"))
}
