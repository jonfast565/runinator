use chrono::Utc;
use runinator_broker::Broker;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::RunStatus,
    workflows::{WorkflowNode, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;

use crate::{api::SchedulerApi, context::*};

pub async fn process_task_node(
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
    let task_run = if let Some(record) = api
        .fetch_idempotency_key(idempotency_scope, &idempotency_key)
        .await?
    {
        let task_run_id = record
            .get("result")
            .and_then(|result| result.get("task_run_id"))
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                Box::new(RuntimeError::new(
                    "workflow.node.idempotency_invalid".into(),
                    format!("Idempotency key {idempotency_key} is missing task_run_id"),
                )) as SendableError
            })?;
        api.fetch_run(task_run_id).await?
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
        crate::iteration::enqueue_task(broker, &task, task_run.id, parameters.clone()).await?;
        task_run
    };

    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(task_run.id),
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

pub async fn process_wait_node(
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

pub async fn process_condition_node(
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
        None,
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

pub async fn process_approval_node(
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

pub async fn process_loop_node(
    api: &SchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
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
    let node_run = if let Some(latest) = latest.filter(|run| run.status == WorkflowStatus::Running) {
        latest.clone()
    } else {
        api.create_workflow_node_run(workflow_run.id, &node.id, parameters.clone())
            .await?
    };
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
        WorkflowStatus::Running,
        None,
        Some(node_run.attempt + 1),
        None,
        Some(output.clone()),
        None,
        Some("loop_iteration".into()),
        None,
    )
    .await?;

    if exhausted {
        transition_from_node(
            api,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output),
            Some("loop_exhausted".into()),
            node_runs,
        )
        .await?;
    } else {
        let return_to = node.transitions.next.clone().unwrap_or(node.id.clone());
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(return_to),
            Some(serde_json::json!({
                "loop": {
                    "index": index,
                    "item": items[index as usize],
                    "return_to": node.id
                }
            })),
            None,
        )
        .await?;
    }
    Ok(())
}

pub async fn process_subflow_node(
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

pub async fn block_node(
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

pub async fn retry_or_transition(
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

pub async fn transition_from_node(
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
