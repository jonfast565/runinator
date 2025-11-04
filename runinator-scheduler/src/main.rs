use std::{sync::Arc, time::Duration};

use log::{error, info};
use runinator_models::errors::SendableError;
use tokio::{sync::Notify, task::JoinHandle};

use runinator_scheduler::{WorkerManager, api::SchedulerApi, config::parse_config, scheduler_loop};
use runinator_utilities::{startup};

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Scheduler")?;

    info!("Parse scheduler config");
    let config = parse_config()?;

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
    let worker_manager_clone = worker_manager.clone();
    let scheduler_task: JoinHandle<Result<(), SendableError>> = tokio::spawn(async move {
        scheduler_loop(
            worker_manager_clone,
            api_clone,
            notify_scheduler,
            &scheduler_config,
        )
        .await;
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
