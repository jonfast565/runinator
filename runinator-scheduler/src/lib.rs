pub mod api;
pub mod config;
mod context;
mod control;
#[cfg(test)]
mod db_extensions;
pub mod debug;
mod iteration;
mod nodes;
mod worker_comm;
pub mod worker_control;
mod workflow;

pub use worker_comm::WorkerManager;

use std::sync::Arc;
use std::time::Duration;

use log::{error, info};
use runinator_broker::Broker;
use tokio::sync::Notify;

use crate::{api::SchedulerApi, config::Config};

pub async fn scheduler_loop(
    broker: Arc<dyn Broker>,
    api: SchedulerApi,
    notify: Arc<Notify>,
    config: &Config,
) {
    loop {
        if let Err(err) = iteration::run_scheduler_iteration(broker.as_ref(), &api, config).await {
            error!("Error during scheduler iteration: {}", err);
        }
        if let Err(err) = workflow::run_workflow_iteration(broker.as_ref(), &api, config).await {
            error!("Error during workflow iteration: {}", err);
        }

        tokio::select! {
            _ = notify.notified() => {
                info!("Shutdown signal received. Exiting scheduler loop.");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(config.scheduler_frequency_seconds)) => {}
        }
    }
}

#[cfg(test)]
mod tests;
