mod config;
mod executor;
mod provider_repository;

use std::{collections::HashMap, sync::Arc, time::Duration};

use config::parse_config;
use log::{error, info, warn};
use runinator_api::{AsyncApiClient, StaticLocator, TaskRunPayload};
use runinator_broker::{Broker, BrokerError, http::client::HttpBroker, in_memory::InMemoryBroker};
use runinator_models::errors::{RuntimeError, SendableError};
use runinator_plugin::{load_libraries_from_path, plugin::Plugin, print_libs};
use runinator_utilities::startup;
use tokio::sync::Notify;

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Worker")?;

    let config = parse_config()?;
    info!("Worker ID: {}", config.worker_id);

    let libraries = Arc::new(load_libraries(&config.dll_path)?);
    let broker = build_broker(&config)?;
    let api_client = build_api_client(&config)?;

    let shutdown = Arc::new(Notify::new());
    let worker_task = {
        let broker = broker.clone();
        let libraries = Arc::clone(&libraries);
        let api_client = api_client.clone();
        let consumer = config.broker_consumer_id.clone();
        let poll_timeout = Duration::from_secs(config.broker_poll_timeout_seconds);
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            if let Err(err) = run_worker_loop(
                broker,
                consumer,
                libraries,
                api_client,
                poll_timeout,
                shutdown,
            )
            .await
            {
                error!("Worker loop terminated with error: {}", err);
            }
        })
    };

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    info!("Shutdown signal received. Stopping worker...");
    shutdown.notify_waiters();

    if let Err(err) = worker_task.await {
        if !err.is_cancelled() {
            error!("Worker task join error: {}", err);
        }
    }

    Ok(())
}

fn load_libraries(path: &str) -> Result<HashMap<String, Plugin>, SendableError> {
    info!("Loading plugins from {}", path);
    let libraries = load_libraries_from_path(path)?;
    print_libs(&libraries);
    Ok(libraries)
}

fn build_broker(config: &config::Config) -> Result<Arc<dyn Broker>, SendableError> {
    match config.broker_backend.as_str() {
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

            Ok(Arc::new(HttpBroker::new(
                url,
                client,
                Duration::from_secs(config.broker_poll_timeout_seconds),
            )))
        }
        "in-memory" => Ok(Arc::new(InMemoryBroker::new())),
        "rabbitmq" | "kafka" => Err(Box::new(RuntimeError::new(
            "worker.broker.backend_not_ready".into(),
            format!(
                "Broker backend '{}' is not implemented yet",
                config.broker_backend
            ),
        ))),
        other => Err(Box::new(RuntimeError::new(
            "worker.broker.unknown_backend".into(),
            format!("Unknown broker backend '{other}'"),
        ))),
    }
}

fn build_api_client(
    config: &config::Config,
) -> Result<AsyncApiClient<StaticLocator>, SendableError> {
    let locator = StaticLocator::new(config.api_base_url.clone());
    AsyncApiClient::new(locator).map_err(|err| {
        Box::new(RuntimeError::new(
            "worker.api.client".into(),
            err.to_string(),
        )) as SendableError
    })
}

async fn run_worker_loop(
    broker: Arc<dyn Broker>,
    consumer_id: String,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    poll_timeout: Duration,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    loop {
        tokio::select! {
            _ = shutdown.notified() => {
                info!("Worker loop shutting down");
                break;
            }
            result = broker.poll(&consumer_id) => {
                let maybe_delivery = result.map_err(|err| broker_error("poll", err))?;
                match maybe_delivery {
                    Some(delivery) => {
                        match process_delivery(
                            &broker,
                            &consumer_id,
                            Arc::clone(&libraries),
                            api_client.clone(),
                            delivery,
                        ).await {
                            Ok(_) => {}
                            Err(err) => {
                                error!("Error processing task: {}", err);
                            }
                        }
                    }
                    None => {
                        tokio::time::sleep(poll_timeout).await;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn process_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    delivery: runinator_broker::BrokerDelivery,
) -> Result<(), SendableError> {
    let command = delivery.command.clone();
    let task = command.task.clone();
    let result = executor::execute_task(libraries, command.command_id, task.clone()).await;

    if result.success {
        if let Some(task_id) = task.id {
            let payload = TaskRunPayload {
                task_id,
                started_at: result.started_at,
                duration_ms: result.duration_ms(),
                message: result.message.clone(),
            };

            if let Err(err) = api_client.log_task_run(&payload).await {
                error!("Failed to record task run for task {}: {}", task_id, err);
                broker
                    .nack(consumer_id, delivery.delivery_id)
                    .await
                    .map_err(|err| broker_error("nack", err))?;
                return Ok(());
            }
        } else {
            warn!("Task result missing ID; skipping run logging");
        }
    } else {
        warn!(
            "Task {} reported failure: {:?}",
            task.id.unwrap_or_default(),
            result.message
        );
    }

    broker
        .ack(consumer_id, delivery.delivery_id)
        .await
        .map_err(|err| broker_error("ack", err))
}

fn broker_error(context: &'static str, err: BrokerError) -> SendableError {
    Box::new(RuntimeError::new(
        format!("worker.broker.{context}"),
        err.to_string(),
    ))
}
