use std::sync::Arc;

use log::{error, info};
use reqwest::Url;
use runinator_api::{
    AsyncApiClient, ReplicaServiceConfig, StaticLocator, register_replica_session,
    spawn_replica_heartbeat_with_telemetry,
};
use runinator_broker::{
    Broker,
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    http::client::HttpBroker,
    in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
};
use runinator_models::errors::SendableError;
use runinator_models::replicas::ReplicaKind;
use runinator_utilities::resource_telemetry::{TelemetryCollector, attributes_with_host_metadata};
use tokio::sync::Notify;

use runinator_utilities::startup;
use runinator_waker::{
    config::{Config, parse_config},
    waker_loop,
};

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    // held for the process lifetime so otel signals flush on shutdown.
    let _telemetry = startup::startup("Runinator Waker")?;

    info!("Parse waker config");
    let config = parse_config()?;
    info!("Waker ID: {}", config.waker_id);

    let broker = build_broker(&config).await?;
    let notify = Arc::new(Notify::new());
    let api_client = AsyncApiClient::with_credentials(
        StaticLocator::new(config.api_base_url.clone()),
        config.api_key.clone(),
    )
    .map_err(|err| runinator_waker::errors::BROKER_CLIENT.error(err))?;
    let _heartbeat = match register_replica_session(
        &api_client,
        ReplicaServiceConfig {
            replica_type: ReplicaKind::Waker,
            instance_id: config.waker_id.clone(),
            display_name: Some(config.waker_id.clone()),
            host: advertise_host(&config.advertise_host),
            port: None,
            base_path: None,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            attributes: attributes_with_host_metadata(&runinator_models::json!({
                "broker_backend": config.broker_backend,
                "broker_client_id": config.broker_client_id,
                "consumer_group": config.waker_consumer_group,
            })),
            heartbeat_interval: std::time::Duration::from_secs(10),
        },
    )
    .await
    {
        Ok(session) => Some(spawn_replica_heartbeat_with_telemetry(
            api_client.clone(),
            session,
            notify.clone(),
            Some(Arc::new(TelemetryCollector::new())),
        )),
        Err(err) => {
            error!("Failed to register waker replica: {}", err);
            None
        }
    };

    runinator_waker::spawn_liveness(&config, notify.clone());

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

// treat a blank advertise host as unset so the replica list omits it rather than storing "".
fn advertise_host(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
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
        "kafka" => runinator_broker::build_kafka_broker(
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
        )
        .map_err(|err| runinator_waker::errors::BROKER_KAFKA.error(err)),
        "rabbitmq" => runinator_broker::build_rabbitmq_broker(
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
        .map_err(|err| runinator_waker::errors::BROKER_RABBITMQ.error(err)),
        other => Err(runinator_waker::errors::BROKER_UNKNOWN_BACKEND.error(format!("'{other}'"))),
    }
}
