use chrono::{DateTime, Utc};
use log::{debug, error};
use runinator_broker::{Broker, BrokerError, BrokerMessage};
use runinator_comm::TaskCommand;
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
    runs::RunStatus,
};
use serde_json::Value;

use crate::{api::SchedulerApi, config::Config, db_extensions};

use uuid::Uuid;

pub async fn run_scheduler_iteration(
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

        let Some(task_id) = task.id else {
            error!("Skipping due task without ID");
            continue;
        };
        let trigger = if force { "immediate" } else { "schedule" };
        let parameters = task.default_parameters.clone();
        let run = match api.create_run(task_id, parameters.clone(), trigger).await {
            Ok(run) => run,
            Err(err) => {
                error!("Failed creating run for task {task_id}: {err}");
                continue;
            }
        };

        match enqueue_task(broker, &task, run.id, parameters).await {
            Ok(_) => {
                debug!("Task {} queued successfully", task.id.unwrap_or_default());
                // After enqueuing, update next execution
                if task.immediate {
                    task.immediate = false;
                }
                if let Err(err) =
                    db_extensions::set_next_execution_with_cron_statement(api, &mut task).await
                {
                    error!(
                        "Failed to update next execution for task {}: {}",
                        task_id, err
                    );
                }
            }
            Err(err) => {
                error!("Failed to enqueue task {}: {}", task_id, err);
            }
        }
    }

    Ok(())
}

pub async fn enqueue_task(
    broker: &dyn Broker,
    task: &ScheduledTask,
    run_id: i64,
    parameters: Value,
) -> Result<(), SendableError> {
    let command = TaskCommand {
        command_id: Uuid::new_v4(),
        run_id: Some(run_id),
        task: task.clone(),
        parameters,
    };
    let message = BrokerMessage {
        command,
        dedupe_key: Some(build_dedupe_key(
            task.id.unwrap_or_default(),
            task.next_execution,
        )),
        enqueued_at: Utc::now(),
    };
    broker
        .publish(message)
        .await
        .map_err(|e| broker_error("enqueue", e))
}

pub async fn enqueue_task_with_dedupe(
    broker: &dyn Broker,
    task: &ScheduledTask,
    run_id: i64,
    parameters: Value,
    dedupe_key: String,
) -> Result<(), SendableError> {
    let command = TaskCommand {
        command_id: Uuid::new_v4(),
        run_id: Some(run_id),
        task: task.clone(),
        parameters,
    };
    let message = BrokerMessage {
        command,
        dedupe_key: Some(dedupe_key),
        enqueued_at: Utc::now(),
    };
    broker
        .publish(message)
        .await
        .map_err(|e| broker_error("enqueue_dedupe", e))
}

pub async fn run_external_run_iteration(
    broker: &dyn Broker,
    api: &SchedulerApi,
) -> Result<(), SendableError> {
    let tasks = api.fetch_tasks().await?;
    for run in api.fetch_runs_by_status(RunStatus::Queued).await? {
        if run.workflow_run_id.is_some() {
            continue;
        }

        let task = match tasks.iter().find(|t| t.id == Some(run.task_id)) {
            Some(task) => task,
            None => {
                error!("Run {} references missing task {}", run.id, run.task_id);
                continue;
            }
        };

        let dedupe_key = format!("run:{}", run.id);
        match enqueue_task_with_dedupe(broker, task, run.id, run.parameters.clone(), dedupe_key)
            .await
        {
            Ok(()) => {}
            Err(err) if is_duplicate_broker_error(err.as_ref()) => {
                debug!("Run {} is already enqueued", run.id);
            }
            Err(err) => {
                error!("Failed to re-enqueue run {}: {}", run.id, err);
            }
        }
    }
    Ok(())
}

pub fn build_dedupe_key(task_id: i64, next_execution: Option<DateTime<Utc>>) -> String {
    format!(
        "{}-{}",
        task_id,
        next_execution.map(|dt| dt.timestamp()).unwrap_or(0)
    )
}

pub fn broker_error(context: &'static str, err: BrokerError) -> SendableError {
    Box::new(RuntimeError::new(
        format!("broker.{}", context),
        err.to_string(),
    ))
}

fn is_duplicate_broker_error(err: &(dyn std::error::Error + 'static)) -> bool {
    err.to_string()
        .starts_with("broker.enqueue_dedupe: duplicate message")
}

pub fn is_task_due(task: &ScheduledTask, force: bool, now: DateTime<Utc>) -> bool {
    if force {
        return true;
    }

    match task.next_execution {
        Some(next_execution) => next_execution <= now,
        None => false,
    }
}
