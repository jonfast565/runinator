pub mod api;
pub mod config;
mod db_extensions;
mod worker_comm;

pub use worker_comm::WorkerManager;

use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use log::{debug, error, info};
use runinator_broker::{Broker, BrokerError, BrokerMessage};
use runinator_comm::TaskCommand;
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
    runs::RunStatus,
    workflows::{WorkflowRun, WorkflowStep, WorkflowStepRun},
};
use serde_json::Value;
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
                if let Err(err) = run_external_run_iteration(broker.as_ref(), &api).await {
                    error!("Error during external run iteration: {}", err);
                }
                if let Err(err) = run_workflow_iteration(broker.as_ref(), &api).await {
                    error!("Error during workflow iteration: {}", err);
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

async fn enqueue_task(
    broker: &dyn Broker,
    task: &ScheduledTask,
    run_id: i64,
    parameters: Value,
) -> Result<(), SendableError> {
    let task_id = task.id.ok_or_else(|| {
        RuntimeError::new(
            "scheduler.task.missing_id".into(),
            "Cannot enqueue task without an ID".into(),
        )
    })?;

    let dedupe_key = build_dedupe_key(task_id, task.next_execution);
    enqueue_task_with_dedupe(broker, task, run_id, parameters, dedupe_key).await
}

async fn enqueue_task_with_dedupe(
    broker: &dyn Broker,
    task: &ScheduledTask,
    run_id: i64,
    parameters: Value,
    dedupe_key: String,
) -> Result<(), SendableError> {
    let _task_id = task.id.ok_or_else(|| {
        RuntimeError::new(
            "scheduler.task.missing_id".into(),
            "Cannot enqueue task without an ID".into(),
        )
    })?;

    let message = BrokerMessage {
        command: TaskCommand {
            command_id: Uuid::new_v4(),
            task: task.clone(),
            run_id: Some(run_id),
            parameters,
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

async fn run_external_run_iteration(
    broker: &dyn Broker,
    api: &SchedulerApi,
) -> Result<(), SendableError> {
    let tasks = api
        .fetch_tasks()
        .await?
        .into_iter()
        .filter_map(|task| task.id.map(|id| (id, task)))
        .collect::<HashMap<_, _>>();

    for run in api.fetch_runs_by_status(RunStatus::Queued).await? {
        if run.workflow_run_id.is_some() {
            continue;
        }
        if matches!(run.trigger.as_str(), "schedule" | "immediate")
            || run.trigger.starts_with("workflow:")
        {
            continue;
        }
        let Some(task) = tasks.get(&run.task_id) else {
            error!(
                "Queued run {} references missing task {}",
                run.id, run.task_id
            );
            continue;
        };
        if !task.enabled {
            error!(
                "Queued run {} references disabled task {}",
                run.id, run.task_id
            );
            continue;
        }
        enqueue_task_with_dedupe(
            broker,
            task,
            run.id,
            run.parameters.clone(),
            format!("run:{}", run.id),
        )
        .await?;
        debug!("External run {} queued for task {}", run.id, run.task_id);
    }
    Ok(())
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

async fn run_workflow_iteration(
    broker: &dyn Broker,
    api: &SchedulerApi,
) -> Result<(), SendableError> {
    for run in api
        .fetch_workflow_runs_by_status(RunStatus::Queued)
        .await?
        .into_iter()
        .chain(
            api.fetch_workflow_runs_by_status(RunStatus::Running)
                .await?,
        )
    {
        process_workflow_run(broker, api, run).await?;
    }
    Ok(())
}

async fn process_workflow_run(
    broker: &dyn Broker,
    api: &SchedulerApi,
    workflow_run: WorkflowRun,
) -> Result<(), SendableError> {
    let workflow = api.fetch_workflow(workflow_run.workflow_id).await?;
    let steps = runinator_workflows::validate_workflow(&workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let (_, mut step_runs) = api.fetch_workflow_run(workflow_run.id).await?;

    if step_runs.is_empty() {
        for step in &steps {
            step_runs.push(
                api.create_workflow_step_run(workflow_run.id, &step.id, step.parameters.clone())
                    .await?,
            );
        }
        api.update_workflow_run(workflow_run.id, RunStatus::Running, None)
            .await?;
    }

    refresh_running_steps(api, &steps, &mut step_runs).await?;

    if step_runs.iter().any(|step| {
        matches!(
            step.status,
            RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
        )
    }) {
        api.update_workflow_run(
            workflow_run.id,
            RunStatus::Failed,
            Some("One or more workflow steps failed".into()),
        )
        .await?;
        return Ok(());
    }

    if !step_runs.is_empty()
        && step_runs
            .iter()
            .all(|step| matches!(step.status, RunStatus::Succeeded))
    {
        api.update_workflow_run(workflow_run.id, RunStatus::Succeeded, None)
            .await?;
        return Ok(());
    }

    let by_id = step_runs
        .iter()
        .map(|step_run| (step_run.step_id.clone(), step_run.clone()))
        .collect::<HashMap<_, _>>();
    let mut running_count = step_runs
        .iter()
        .filter(|step| step.status == RunStatus::Running)
        .count();
    let concurrency = runinator_workflows::workflow_concurrency(&workflow);
    for step in steps {
        if running_count >= concurrency {
            break;
        }
        let Some(step_run) = by_id.get(&step.id) else {
            continue;
        };
        if step_run.status != RunStatus::Queued {
            continue;
        }
        if !dependencies_succeeded(&step, &by_id) {
            continue;
        }

        let task = api
            .fetch_tasks()
            .await?
            .into_iter()
            .find(|task| task.id == Some(step.task_id))
            .ok_or_else(|| {
                Box::new(RuntimeError::new(
                    "workflow.step.task_not_found".into(),
                    format!(
                        "Task {} not found for workflow step {}",
                        step.task_id, step.id
                    ),
                )) as SendableError
            })?;
        let parameters = build_step_parameters(api, &task, &step, &by_id).await?;
        let task_run = api
            .create_run(
                step.task_id,
                parameters.clone(),
                format!("workflow:{}", workflow_run.id),
            )
            .await?;
        enqueue_task(broker, &task, task_run.id, parameters).await?;
        let attempt = step_run.attempt + 1;
        api.update_workflow_step_run(
            step_run.id,
            RunStatus::Running,
            Some(task_run.id),
            Some(attempt),
            None,
            None,
        )
        .await?;
        running_count += 1;
    }

    Ok(())
}

async fn refresh_running_steps(
    api: &SchedulerApi,
    steps: &[WorkflowStep],
    step_runs: &mut [WorkflowStepRun],
) -> Result<(), SendableError> {
    for step_run in step_runs.iter_mut() {
        if step_run.status != RunStatus::Running {
            continue;
        }
        let Some(task_run_id) = step_run.task_run_id else {
            continue;
        };
        let task_run = api.fetch_run(task_run_id).await?;
        let step = steps.iter().find(|step| step.id == step_run.step_id);
        if let (Some(step), Some(started_at)) = (step, step_run.started_at) {
            let timeout = step.timeout_seconds.or(step.timeout);
            if timeout
                .is_some_and(|timeout| Utc::now() - started_at > chrono::Duration::seconds(timeout))
            {
                let status =
                    retry_or_fail(api, step, step_run, Some("Step timed out".into())).await?;
                step_run.status = status;
                continue;
            }
        }
        match task_run.status {
            RunStatus::Succeeded => {
                api.update_workflow_step_run(
                    step_run.id,
                    RunStatus::Succeeded,
                    None,
                    None,
                    None,
                    None,
                )
                .await?;
                step_run.status = RunStatus::Succeeded;
            }
            RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled => {
                if let Some(step) = step {
                    step_run.status =
                        retry_or_fail(api, step, step_run, task_run.message.clone()).await?;
                } else {
                    api.update_workflow_step_run(
                        step_run.id,
                        task_run.status,
                        None,
                        None,
                        None,
                        task_run.message.clone(),
                    )
                    .await?;
                    step_run.status = task_run.status;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn retry_or_fail(
    api: &SchedulerApi,
    step: &WorkflowStep,
    step_run: &WorkflowStepRun,
    message: Option<String>,
) -> Result<RunStatus, SendableError> {
    if step_run.attempt < step.retry.max_attempts {
        api.update_workflow_step_run(step_run.id, RunStatus::Queued, None, None, None, message)
            .await?;
        Ok(RunStatus::Queued)
    } else {
        api.update_workflow_step_run(step_run.id, RunStatus::Failed, None, None, None, message)
            .await?;
        Ok(RunStatus::Failed)
    }
}

async fn build_step_parameters(
    api: &SchedulerApi,
    task: &ScheduledTask,
    step: &WorkflowStep,
    by_id: &HashMap<String, WorkflowStepRun>,
) -> Result<Value, SendableError> {
    let base = merge_parameters(&task.default_parameters, &step.parameters);
    let mut upstream_outputs = HashMap::new();
    for mapping in &step.mappings {
        let Some(step_run) = by_id.get(&mapping.from_step) else {
            continue;
        };
        let Some(task_run_id) = step_run.task_run_id else {
            continue;
        };
        let run = api.fetch_run(task_run_id).await?;
        if let Some(output) = run.output_json {
            upstream_outputs.insert(mapping.from_step.clone(), output);
        }
    }
    runinator_workflows::apply_mappings(&base, step, &upstream_outputs)
        .map_err(|err| -> SendableError { Box::new(err) })
}

fn dependencies_succeeded(step: &WorkflowStep, by_id: &HashMap<String, WorkflowStepRun>) -> bool {
    step.needs.iter().all(|dependency| {
        by_id
            .get(dependency)
            .map(|step_run| step_run.status == RunStatus::Succeeded)
            .unwrap_or(false)
    })
}

fn merge_parameters(defaults: &Value, parameters: &Value) -> Value {
    match (defaults, parameters) {
        (Value::Object(defaults), Value::Object(parameters)) => {
            let mut merged = defaults.clone();
            for (key, value) in parameters {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        (_, Value::Null) => defaults.clone(),
        _ => parameters.clone(),
    }
}
