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
    api::SchedulerApi,
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

    info!("Initializing worker discovery");
    let worker_manager = match WorkerManager::new(&config).await {
        Ok(manager) => Arc::new(manager),
        Err(err) => {
            error!("Unable to initialize worker manager: {}", err);
            return Err(err);
        }
    };

    let api_timeout = Duration::from_secs(config.api_timeout_seconds);
    info!("Preparing scheduler API client");
    let api = SchedulerApi::new(worker_manager.clone(), api_timeout)?;
    let worker_control_task =
        worker_control::spawn_listener(&config, Arc::new(api.clone()), notify.clone()).await?;

    if worker_manager.current_service_url().await.is_none() {
        info!("Waiting for Runinator web service discovery via gossip...");
    }

    info!("Starting scheduler loop");
    let notify_scheduler = notify.clone();
    let scheduler_config = config.clone();
    let api_clone = api.clone();
    let broker_clone = broker.clone();
    let scheduler_task: JoinHandle<Result<(), SendableError>> = tokio::spawn(async move {
        scheduler_loop(broker_clone, api_clone, notify_scheduler, &scheduler_config).await;
        Ok(())
    });

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
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
