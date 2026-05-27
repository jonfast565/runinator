use std::{sync::Arc, time::Duration};

use log::{error, info};
use reqwest::Url;
use runinator_broker::{
    Broker,
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    http::client::HttpBroker,
    in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
};
use runinator_models::errors::{RuntimeError, SendableError};
use tokio::{sync::Notify, task::JoinHandle};

use runinator_scheduler::{
    WorkerManager,
    api::{SchedulerApi, SchedulerServiceLocator},
    config::{Config, parse_config},
    scheduler_loop, worker_control,
};
use runinator_utilities::startup;

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Scheduler")?;

    info!("Parse scheduler config");
    let config = parse_config()?;

    let broker = build_broker(&config).await?;

    let notify = Arc::new(Notify::new());

    let api_timeout = Duration::from_secs(config.api_timeout_seconds);
    info!("Preparing scheduler API client");
    let locator = build_service_locator(&config).await?;
    let api = SchedulerApi::new(locator, api_timeout)?;
    let worker_control_task =
        worker_control::spawn_listener(&config, Arc::new(api.clone()), notify.clone()).await?;

    info!("Starting scheduler loop");
    let notify_scheduler = notify.clone();
    let scheduler_config = config.clone();
    let api_clone = api.clone();
    let broker_clone = broker.clone();
    let scheduler_task: JoinHandle<Result<(), SendableError>> = tokio::spawn(async move {
        scheduler_loop(broker_clone, api_clone, notify_scheduler, &scheduler_config).await;
        Ok(())
    });

    tokio::signal::ctrl_c().await.map_err(|err| {
        Box::new(RuntimeError::new(
            "scheduler.signal.ctrl_c".into(),
            format!("Failed to listen for Ctrl+C: {err}"),
        )) as SendableError
    })?;
    info!("Received shutdown signal. Shutting down...");
    notify.notify_waiters();

    if let Err(e) = tokio::try_join!(scheduler_task) {
        error!("Error while shutting down scheduler: {:?}", e);
    }
    if let Some(worker_control_task) = worker_control_task {
        match worker_control_task.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => error!("Worker-control listener terminated with error: {}", err),
            Err(err) if err.is_cancelled() => {}
            Err(err) => error!("Worker-control listener join error: {}", err),
        }
    }

    info!("Scheduler shutdown complete.");
    Ok(())
}

async fn build_service_locator(config: &Config) -> Result<SchedulerServiceLocator, SendableError> {
    if let Some(base_url) = non_empty_api_base_url(config) {
        info!("Using configured Runinator web service URL: {base_url}");
        return Ok(SchedulerServiceLocator::Static(
            runinator_api::StaticLocator::new(base_url.to_string()),
        ));
    }

    info!("Initializing web service discovery via gossip");
    let worker_manager = match WorkerManager::new(config).await {
        Ok(manager) => manager,
        Err(err) => {
            error!("Unable to initialize worker manager: {}", err);
            return Err(err);
        }
    };

    if worker_manager.current_service_url().await.is_none() {
        info!("Waiting for Runinator web service discovery via gossip...");
    }

    Ok(SchedulerServiceLocator::Gossip(worker_manager))
}

fn non_empty_api_base_url(config: &Config) -> Option<&str> {
    config
        .api_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn build_broker(config: &Config) -> Result<Arc<dyn Broker>, SendableError> {
    match config.broker_backend.as_str() {
        "http" => {
            let url = Url::parse(&config.broker_endpoint).map_err(|err| -> SendableError {
                Box::new(RuntimeError::new(
                    "scheduler.broker.invalid_endpoint".into(),
                    err.to_string().into(),
                ))
            })?;
            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| -> SendableError {
                    Box::new(RuntimeError::new(
                        "scheduler.broker.client".into(),
                        err.to_string().into(),
                    ))
                })?;

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
                    .with_client_id(config.broker_client_id.clone()),
            )
            .await
        }
        other => Err(Box::new(RuntimeError::new(
            "scheduler.broker.unknown_backend".into(),
            format!("Unknown broker backend '{other}'").into(),
        ))),
    }
}

#[cfg(feature = "kafka")]
fn build_kafka_broker(config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::kafka::KafkaBroker::new(config).map_err(
        |err| -> SendableError {
            Box::new(RuntimeError::new(
                "scheduler.broker.kafka".into(),
                err.to_string().into(),
            ))
        },
    )?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "kafka"))]
fn build_kafka_broker(_config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    Err(Box::new(RuntimeError::new(
        "scheduler.broker.kafka_feature_disabled".into(),
        "Broker backend 'kafka' requires building runinator-scheduler with --features kafka".into(),
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
                "scheduler.broker.rabbitmq".into(),
                err.to_string().into(),
            ))
        })?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "rabbitmq"))]
async fn build_rabbitmq_broker(
    _config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    Err(Box::new(RuntimeError::new(
        "scheduler.broker.rabbitmq_feature_disabled".into(),
        "Broker backend 'rabbitmq' requires building runinator-scheduler with --features rabbitmq"
            .into(),
    )))
}

#[cfg(test)]
mod service_locator_tests {
    use super::*;

    #[tokio::test]
    async fn configured_api_base_url_uses_static_locator() {
        let config = test_config(Some(
            "http://runinator-ws.runinator.svc.cluster.local:8080/".into(),
        ));

        let locator = build_service_locator(&config).await.unwrap();

        assert!(matches!(locator, SchedulerServiceLocator::Static(_)));
    }

    #[tokio::test]
    async fn missing_api_base_url_uses_gossip_locator() {
        let config = test_config(None);

        let locator = build_service_locator(&config).await.unwrap();

        assert!(matches!(locator, SchedulerServiceLocator::Gossip(_)));
    }

    fn test_config(api_base_url: Option<String>) -> Config {
        Config {
            scheduler_frequency_seconds: 1,
            scheduler_id: "test-scheduler".into(),
            scheduler_lease_seconds: 60,
            scheduler_claim_limit: 50,
            gossip_bind: "127.0.0.1".into(),
            gossip_port: 0,
            gossip_targets: Vec::new(),
            api_base_url,
            api_timeout_seconds: 30,
            broker_backend: "in-memory".into(),
            broker_endpoint: "127.0.0.1:7070".into(),
            broker_action_topic: "runinator.actions".into(),
            broker_control_topic: "runinator.control".into(),
            broker_result_topic: "runinator.results".into(),
            broker_client_id: "runinator-scheduler".into(),
            worker_control_transport: "disabled".into(),
            worker_control_bind: "127.0.0.1".into(),
            worker_control_port: 7080,
        }
    }
}
