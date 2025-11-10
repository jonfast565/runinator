use std::{sync::Arc, time::Duration};

use log::{error, info};
use reqwest::Url;
use runinator_broker::{Broker, http::client::HttpBroker, in_memory::InMemoryBroker};
use runinator_models::errors::{RuntimeError, SendableError};
use tokio::{sync::Notify, task::JoinHandle};

use runinator_scheduler::{
    WorkerManager,
    api::SchedulerApi,
    config::{Config, parse_config},
    scheduler_loop,
};
use runinator_utilities::startup;

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Scheduler")?;

    info!("Parse scheduler config");
    let config = parse_config()?;

    let broker = build_broker(&config)?;

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

    info!("Scheduler shutdown complete.");
    Ok(())
}

fn build_broker(config: &Config) -> Result<Arc<dyn Broker>, SendableError> {
    match config.broker_backend.as_str() {
        "http" => {
            let url = Url::parse(&config.broker_endpoint).map_err(|err| -> SendableError {
                Box::new(RuntimeError::new(
                    "scheduler.broker.invalid_endpoint".into(),
                    err.to_string().into(),
                ))
            })?;
            let poll_timeout = Duration::from_secs(config.broker_poll_timeout_seconds);
            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| -> SendableError {
                    Box::new(RuntimeError::new(
                        "scheduler.broker.client".into(),
                        err.to_string().into(),
                    ))
                })?;

            Ok(Arc::new(HttpBroker::new(url, client, poll_timeout)))
        }
        "in-memory" => Ok(Arc::new(InMemoryBroker::new())),
        "rabbitmq" | "kafka" => Err(Box::new(RuntimeError::new(
            "scheduler.broker.backend_not_ready".into(),
            format!(
                "Broker backend '{}' is not implemented yet",
                config.broker_backend
            )
            .into(),
        ))),
        other => Err(Box::new(RuntimeError::new(
            "scheduler.broker.unknown_backend".into(),
            format!("Unknown broker backend '{other}'").into(),
        ))),
    }
}
