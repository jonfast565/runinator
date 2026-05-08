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
    workflows::{WorkflowNode, WorkflowNodeKind, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
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
    for status in [
        WorkflowStatus::Queued,
        WorkflowStatus::Running,
        WorkflowStatus::Waiting,
        WorkflowStatus::ApprovalRequired,
        WorkflowStatus::Blocked,
    ] {
        for run in api.fetch_workflow_runs_by_status(status).await? {
            process_workflow_run(broker, api, run).await?;
        }
    }
    Ok(())
}

async fn process_workflow_run(
    broker: &dyn Broker,
    api: &SchedulerApi,
    workflow_run: WorkflowRun,
) -> Result<(), SendableError> {
    let workflow = api.fetch_workflow(workflow_run.workflow_id).await?;
    let (start, nodes) = runinator_workflows::validate_workflow(&workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let (_, node_runs) = api.fetch_workflow_run(workflow_run.id).await?;
    let active_node_id = workflow_run
        .active_node_id
        .clone()
        .unwrap_or_else(|| start.clone());
    let node_by_id = nodes
        .into_iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    let Some(node) = node_by_id.get(&active_node_id) else {
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::Failed,
            Some(active_node_id),
            None,
            Some("Active workflow node is missing".into()),
        )
        .await?;
        return Ok(());
    };
    let latest = latest_node_run(&node_runs, &active_node_id);
    match node.kind {
        WorkflowNodeKind::Task => {
            process_task_node(broker, api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Wait => {
            process_wait_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Condition => {
            process_condition_node(api, &workflow_run, node, &node_runs).await?
        }
        WorkflowNodeKind::Approval => {
            process_approval_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Loop => process_loop_node(api, &workflow_run, node, &node_runs).await?,
        WorkflowNodeKind::Subflow => {
            process_subflow_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::End => {
            if let Some(loop_node) = workflow_run
                .state
                .pointer("/loop/return_to")
                .and_then(Value::as_str)
            {
                api.update_workflow_run(
                    workflow_run.id,
                    WorkflowStatus::Running,
                    Some(loop_node.to_string()),
                    Some(serde_json::json!({ "loop": {} })),
                    None,
                )
                .await?;
                return Ok(());
            }
            api.update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Succeeded,
                Some(node.id.clone()),
                None,
                None,
            )
            .await?;
        }
    };

    Ok(())
}

async fn process_task_node(
    broker: &dyn Broker,
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Running) {
        let Some(task_run_id) = node_run.task_run_id else {
            return Ok(());
        };
        let task_run = api.fetch_run(task_run_id).await?;
        if let (Some(timeout), Some(started_at)) = (node.timeout_seconds, node_run.started_at) {
            if Utc::now() - started_at > chrono::Duration::seconds(timeout) {
                retry_or_transition(
                    api,
                    workflow_run,
                    node,
                    node_run,
                    WorkflowStatus::TimedOut,
                    None,
                    Some("Node timed out".into()),
                    node_runs,
                )
                .await?;
                return Ok(());
            }
        }
        match task_run.status {
            RunStatus::Succeeded => {
                transition_from_node(
                    api,
                    workflow_run,
                    node,
                    node_run,
                    WorkflowStatus::Succeeded,
                    task_run.output_json,
                    None,
                    node_runs,
                )
                .await?;
            }
            RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled => {
                let status = match task_run.status {
                    RunStatus::TimedOut => WorkflowStatus::TimedOut,
                    RunStatus::Canceled => WorkflowStatus::Canceled,
                    _ => WorkflowStatus::Failed,
                };
                retry_or_transition(
                    api,
                    workflow_run,
                    node,
                    node_run,
                    status,
                    task_run.output_json,
                    task_run.message,
                    node_runs,
                )
                .await?;
            }
            _ => {}
        }
        return Ok(());
    }

    if latest.is_some_and(|run| {
        matches!(
            run.status,
            WorkflowStatus::Waiting | WorkflowStatus::ApprovalRequired
        )
    }) {
        return Ok(());
    }

    let task_id = node.task_id.ok_or_else(|| {
        Box::new(RuntimeError::new(
            "workflow.node.task_missing_id".into(),
            format!("Task node {} has no task_id", node.id),
        )) as SendableError
    })?;
    let task = api
        .fetch_tasks()
        .await?
        .into_iter()
        .find(|task| task.id == Some(task_id))
        .ok_or_else(|| {
            Box::new(RuntimeError::new(
                "workflow.node.task_not_found".into(),
                format!("Task {task_id} not found for workflow node {}", node.id),
            )) as SendableError
        })?;
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    let parameters = build_node_parameters(&task, node, workflow_run, node_runs)?;
    let attempt = node_run.attempt + 1;
    let idempotency_scope = "workflow_task_node";
    let idempotency_key = format!("{}:{}:{}", workflow_run.id, node.id, attempt);
    let task_run_id = if let Some(record) = api
        .fetch_idempotency_key(idempotency_scope, &idempotency_key)
        .await?
    {
        record
            .get("result")
            .and_then(|result| result.get("task_run_id"))
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                Box::new(RuntimeError::new(
                    "workflow.node.idempotency_invalid".into(),
                    format!("Idempotency key {idempotency_key} is missing task_run_id"),
                )) as SendableError
            })?
    } else {
        let task_run = api
            .create_run(
                task_id,
                parameters.clone(),
                format!("workflow:{}", workflow_run.id),
            )
            .await?;
        api.put_idempotency_key(
            idempotency_scope,
            &idempotency_key,
            serde_json::json!({ "task_run_id": task_run.id }),
        )
        .await?;
        enqueue_task(broker, &task, task_run.id, parameters.clone()).await?;
        task_run.id
    };
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(task_run_id),
        Some(attempt),
        Some(parameters),
        None,
        None,
        Some("task_started".into()),
        None,
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(node.id.clone()),
        None,
        None,
    )
    .await?;
    Ok(())
}

async fn process_wait_node(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::Waiting {
            if let Some(expected) = node.wait.get("until_status").and_then(Value::as_str) {
                let current = node_run
                    .state
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if current == expected {
                    transition_from_node(
                        api,
                        workflow_run,
                        node,
                        node_run,
                        WorkflowStatus::Succeeded,
                        Some(node_run.state.clone()),
                        Some("wait_status_matched".into()),
                        node_runs,
                    )
                    .await?;
                }
                return Ok(());
            }
            let deadline = node_run
                .state
                .get("deadline_unix")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            if Utc::now().timestamp() < deadline {
                return Ok(());
            }
            transition_from_node(
                api,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                Some(serde_json::json!({ "deadline_unix": deadline })),
                Some("wait_elapsed".into()),
                node_runs,
            )
            .await?;
            return Ok(());
        }
    }
    let seconds = node
        .wait
        .get("seconds")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let deadline = Utc::now().timestamp() + seconds;
    let state = serde_json::json!({
        "deadline_unix": deadline,
        "status": node.wait.get("initial_status").and_then(Value::as_str).unwrap_or("waiting")
    });
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        None,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.clone()),
        Some("wait_started".into()),
        None,
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        Some(state),
        None,
    )
    .await?;
    Ok(())
}

async fn process_condition_node(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    let context = runtime_context(workflow_run, node_runs);
    let matched = runinator_workflows::evaluate_condition(&node.condition, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let status = if matched {
        WorkflowStatus::Succeeded
    } else {
        WorkflowStatus::Blocked
    };
    transition_from_node(
        api,
        workflow_run,
        node,
        &node_run,
        status,
        Some(serde_json::json!({ "matched": matched })),
        Some(
            if matched {
                "condition_matched"
            } else {
                "condition_unmatched"
            }
            .into(),
        ),
        node_runs,
    )
    .await
}

async fn process_approval_node(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::Succeeded {
            transition_from_node(
                api,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                node_run.output_json.clone(),
                Some("approval_resolved".into()),
                node_runs,
            )
            .await?;
        }
        return Ok(());
    }
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    let approval = api
        .create_automation_record(
            "/approvals",
            serde_json::json!({
                "workflow_run_id": workflow_run.id,
                "node_id": node.id,
                "approval_type": node.parameters.get("approval_type").and_then(Value::as_str).unwrap_or("generic"),
                "prompt": node.parameters.get("prompt").and_then(Value::as_str).unwrap_or("Approval required"),
                "status": "pending",
                "provider": "runinator",
                "resource_type": "approval_request",
                "external_id": format!("workflow:{}:node:{}", workflow_run.id, node.id),
                "metadata": node.parameters,
            }),
        )
        .await?;
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::ApprovalRequired,
        None,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(serde_json::json!({
            "approval": node.parameters,
            "approval_id": approval.get("id").and_then(Value::as_i64)
        })),
        Some("approval_required".into()),
        None,
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::ApprovalRequired,
        Some(node.id.clone()),
        None,
        None,
    )
    .await
}

async fn process_loop_node(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let context = runtime_context(workflow_run, node_runs);
    let parameters = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let items = parameters
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let prior_iterations = node_runs
        .iter()
        .filter(|run| run.node_id == node.id && run.status == WorkflowStatus::Succeeded)
        .count() as i64;
    let max_iterations = node.max_iterations.unwrap_or(i64::MAX).max(0);
    let index = prior_iterations;
    let exhausted = index >= items.len() as i64 || index >= max_iterations;
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, parameters.clone())
        .await?;
    let output = if exhausted {
        serde_json::json!({
            "index": index,
            "has_next": false,
            "count": items.len()
        })
    } else {
        serde_json::json!({
            "index": index,
            "item": items[index as usize],
            "has_next": true,
            "count": items.len()
        })
    };
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        None,
        Some(node_run.attempt + 1),
        None,
        Some(output.clone()),
        Some(serde_json::json!({ "index": index, "exhausted": exhausted })),
        Some(if exhausted {
            "loop_exhausted".into()
        } else {
            "loop_next".into()
        }),
        None,
    )
    .await?;

    let next = if exhausted {
        node.transitions
            .on_success
            .clone()
            .or_else(|| node.transitions.next.clone())
    } else {
        node.transitions.next.clone()
    };
    match next {
        Some(next) => {
            api.update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Running,
                Some(next),
                Some(serde_json::json!({
                    "loop": {
                        "return_to": node.id,
                        node.id.clone(): output
                    }
                })),
                None,
            )
            .await
        }
        None => {
            api.update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Succeeded,
                Some(node.id.clone()),
                None,
                None,
            )
            .await
        }
    }
}

async fn process_subflow_node(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let Some(subflow_id) = node.subflow_id else {
        return block_node(
            api,
            workflow_run,
            node,
            "Subflow node is missing subflow_id",
        )
        .await;
    };

    if let Some(node_run) = latest {
        if let Some(subflow_run_id) = node_run.state.get("subflow_run_id").and_then(Value::as_i64) {
            let (subflow_run, _) = api.fetch_workflow_run(subflow_run_id).await?;
            match subflow_run.status {
                WorkflowStatus::Succeeded => {
                    return transition_from_node(
                        api,
                        workflow_run,
                        node,
                        node_run,
                        WorkflowStatus::Succeeded,
                        Some(serde_json::json!({
                            "subflow_run_id": subflow_run_id,
                            "status": subflow_run.status.as_str(),
                            "state": subflow_run.state,
                            "parameters": subflow_run.parameters
                        })),
                        Some("subflow_succeeded".into()),
                        node_runs,
                    )
                    .await;
                }
                WorkflowStatus::Failed
                | WorkflowStatus::TimedOut
                | WorkflowStatus::Canceled
                | WorkflowStatus::Blocked => {
                    return transition_from_node(
                        api,
                        workflow_run,
                        node,
                        node_run,
                        WorkflowStatus::Failed,
                        Some(serde_json::json!({
                            "subflow_run_id": subflow_run_id,
                            "status": subflow_run.status.as_str()
                        })),
                        subflow_run
                            .message
                            .or(Some("Subflow did not succeed".into())),
                        node_runs,
                    )
                    .await;
                }
                _ => return Ok(()),
            }
        }
    }

    let context = runtime_context(workflow_run, node_runs);
    let parameters = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let subflow_run = api
        .create_workflow_run(subflow_id, parameters.clone())
        .await?;
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, parameters)
        .await?;
    let state = serde_json::json!({ "subflow_run_id": subflow_run.id });
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        None,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.clone()),
        Some("subflow_started".into()),
        None,
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        Some(state),
        None,
    )
    .await
}

async fn block_node(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    message: &str,
) -> Result<(), SendableError> {
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Blocked,
        None,
        Some(node_run.attempt + 1),
        None,
        None,
        None,
        Some("blocked".into()),
        Some(message.into()),
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Blocked,
        Some(node.id.clone()),
        None,
        Some(message.into()),
    )
    .await
}

async fn retry_or_transition(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if node_run.attempt < node.retry.max_attempts {
        api.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Queued,
            None,
            None,
            None,
            output_json,
            None,
            Some("retry_queued".into()),
            message,
        )
        .await?;
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(node.id.clone()),
            None,
            None,
        )
        .await
    } else {
        transition_from_node(
            api,
            workflow_run,
            node,
            node_run,
            status,
            output_json,
            message,
            node_runs,
        )
        .await
    }
}

async fn transition_from_node(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    api.update_workflow_node_run(
        node_run.id,
        status,
        None,
        None,
        None,
        output_json.clone(),
        None,
        Some(status.as_str().into()),
        message.clone(),
    )
    .await?;
    let mut context = runtime_context(workflow_run, node_runs);
    if let Some(output) = output_json {
        context
            .pointer_mut(&format!("/steps/{}/output", node.id))
            .map(|slot| *slot = output);
    }
    let next = runinator_workflows::next_transition(node, status, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    match next {
        Some(next) => {
            api.update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Running,
                Some(next),
                None,
                message,
            )
            .await
        }
        None if status == WorkflowStatus::Succeeded => {
            api.update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Succeeded,
                Some(node.id.clone()),
                None,
                message,
            )
            .await
        }
        None => {
            api.update_workflow_run(
                workflow_run.id,
                status,
                Some(node.id.clone()),
                None,
                message,
            )
            .await
        }
    }
}

fn latest_node_run<'a>(
    node_runs: &'a [WorkflowNodeRun],
    node_id: &str,
) -> Option<&'a WorkflowNodeRun> {
    node_runs
        .iter()
        .filter(|run| run.node_id == node_id)
        .max_by_key(|run| run.id)
}

fn build_node_parameters(
    task: &ScheduledTask,
    node: &WorkflowNode,
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
) -> Result<Value, SendableError> {
    let base = merge_parameters(&task.default_parameters, &node.parameters);
    let context = runtime_context(workflow_run, node_runs);
    runinator_workflows::resolve_value_refs(&base, &context)
        .map_err(|err| -> SendableError { Box::new(err) })
}

fn runtime_context(workflow_run: &WorkflowRun, node_runs: &[WorkflowNodeRun]) -> Value {
    let outputs = node_runs
        .iter()
        .filter_map(|run| {
            run.output_json
                .clone()
                .map(|output| (run.node_id.clone(), output))
        })
        .collect::<HashMap<_, _>>();
    let mut context = runinator_workflows::outputs_context(&workflow_run.parameters, &outputs);
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "workflow".into(),
            serde_json::json!({
                "run_id": workflow_run.id,
                "workflow_id": workflow_run.workflow_id,
                "state": workflow_run.state,
            }),
        );
    }
    context
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
