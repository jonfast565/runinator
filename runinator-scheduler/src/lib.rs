pub mod api;
pub mod config;
mod db_extensions;
mod worker_comm;

pub use worker_comm::WorkerManager;

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use runinator_comm::TaskResult;
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
};
use tokio::sync::Notify;

use crate::{api::SchedulerApi, config::Config};

pub async fn scheduler_loop(
    worker_manager: Arc<WorkerManager>,
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
                if let Err(err) = run_scheduler_iteration(&api, worker_manager.as_ref(), config).await {
                    error!("Error during scheduler iteration: {}", err);
                }
            }
        }
    }
}

async fn run_scheduler_iteration(
    api: &SchedulerApi,
    worker_manager: &WorkerManager,
    config: &Config,
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

        match dispatch_to_worker(api, worker_manager, &task, config).await {
            Ok(_) => {
                debug!(
                    "Task {} dispatched successfully",
                    task.id.unwrap_or_default()
                );
            }
            Err(err) => {
                error!(
                    "Failed dispatching task {}: {}",
                    task.id.unwrap_or_default(),
                    err
                );
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

async fn dispatch_to_worker(
    api: &SchedulerApi,
    worker_manager: &WorkerManager,
    task: &ScheduledTask,
    config: &Config,
) -> Result<(), SendableError> {
    if task.id.is_none() {
        return Err(Box::new(RuntimeError::new(
            "scheduler.task.missing_id".into(),
            "Cannot dispatch task without an ID".into(),
        )));
    }

    let timeout = Duration::from_secs(config.worker_timeout_seconds);
    let retries = config.worker_command_retry;

    match worker_manager.dispatch_task(task, timeout, retries).await {
        Ok(result) => handle_worker_result(api, task, result).await,
        Err(err) => Err(err),
    }
}

async fn handle_worker_result(
    api: &SchedulerApi,
    task: &ScheduledTask,
    result: TaskResult,
) -> Result<(), SendableError> {
    if !result.success {
        warn!(
            "Worker reported failure for task {}: {:?}",
            task.id.unwrap_or_default(),
            result.message
        );
        return Ok(());
    }

    let task_id = task.id.ok_or_else(|| {
        RuntimeError::new(
            "scheduler.task.missing_id".into(),
            "Task missing identifier when logging result".into(),
        )
    })?;

    api.log_task_run(
        task_id,
        result.started_at,
        result.duration_ms(),
        result.message.clone(),
    )
    .await?;
    Ok(())
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
