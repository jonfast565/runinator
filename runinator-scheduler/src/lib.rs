pub mod api;
pub mod config;
mod db_extensions;
mod worker_comm;

pub use worker_comm::WorkerManager;

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use log::{debug, error, info};
use runinator_broker::{Broker, BrokerError, BrokerMessage};
use runinator_comm::TaskCommand;
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{api::SchedulerApi, config::Config};

pub async fn scheduler_loop(
    broker: Arc<dyn Broker>,
    api: SchedulerApi,
    notify: Arc<Notify>,
    config: &Config,
) {
    loop {
        tokio::select! {
            _ = notify.notified() => {
                info!("Shutdown signal received. Exiting scheduler loop.");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(config.scheduler_frequency_seconds)) => {
                if let Err(err) = run_scheduler_iteration(broker.as_ref(), &api, config).await {
                    error!("Error during scheduler iteration: {}", err);
                }
            }
        }
    }
}

async fn run_scheduler_iteration(
    broker: &dyn Broker,
    api: &SchedulerApi,
    _config: &Config,
) -> Result<(), SendableError> {
    let tasks = api.fetch_tasks().await?;
    let now = Utc::now();

    debug!("Scheduler evaluating {} task(s)", tasks.len());

    for mut task in tasks {
        if !task.enabled {
            continue;
        }

        if task.next_execution.is_none() {
            debug!(
                "Task {} has no next_execution. Initializing.",
                task.id.unwrap_or_default()
            );
            db_extensions::set_initial_execution(api, &mut task).await?;
            continue;
        }

        let force = task.immediate;
        if !is_task_due(&task, force, now) {
            continue;
        }

        if let (Some(start), Some(end)) = (task.blackout_start, task.blackout_end) {
            if now >= start && now <= end {
                debug!(
                    "Task {} is within blackout window. Deferring to {}.",
                    task.id.unwrap_or_default(),
                    end
                );
                task.next_execution = Some(end);
                api.update_task(&task).await?;
                continue;
            }
        }

        match enqueue_task(broker, &task).await {
            Ok(_) => {
                debug!("Task {} queued successfully", task.id.unwrap_or_default());
            }
            Err(err) => {
                error!(
                    "Failed queueing task {}: {}",
                    task.id.unwrap_or_default(),
                    err
                );
                continue;
            }
        }

        if force {
            task.immediate = false;
        }

        if let Err(err) =
            db_extensions::set_next_execution_with_cron_statement(api, &mut task).await
        {
            error!(
                "Unable to update next execution for task {}: {}",
                task.id.unwrap_or_default(),
                err
            );
        }
    }

    Ok(())
}

async fn enqueue_task(broker: &dyn Broker, task: &ScheduledTask) -> Result<(), SendableError> {
    let task_id = task.id.ok_or_else(|| {
        RuntimeError::new(
            "scheduler.task.missing_id".into(),
            "Cannot enqueue task without an ID".into(),
        )
    })?;

    let dedupe_key = build_dedupe_key(task_id, task.next_execution);
    let message = BrokerMessage {
        command: TaskCommand {
            command_id: Uuid::new_v4(),
            task: task.clone(),
        },
        dedupe_key: Some(dedupe_key),
        enqueued_at: Utc::now(),
    };

    match broker.publish(message).await {
        Ok(_) => Ok(()),
        Err(BrokerError::Duplicate(_)) => Ok(()),
        Err(err) => Err(broker_error("publish", err)),
    }
}

fn build_dedupe_key(task_id: i64, next_execution: Option<DateTime<Utc>>) -> String {
    match next_execution {
        Some(next) => format!("{}:{}", task_id, next.timestamp()),
        None => format!("{}:{}", task_id, Utc::now().timestamp()),
    }
}

fn broker_error(context: &'static str, err: BrokerError) -> SendableError {
    Box::new(RuntimeError::new(
        format!("scheduler.broker.{context}"),
        err.to_string(),
    ))
}

fn is_task_due(task: &ScheduledTask, force: bool, now: DateTime<Utc>) -> bool {
    if force {
        return true;
    }

    match task.next_execution {
        Some(next_execution) => next_execution <= now,
        None => false,
    }
}
