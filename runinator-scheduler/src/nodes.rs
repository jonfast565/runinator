use chrono::Utc;
use runinator_broker::Broker;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    workflows::{WorkflowNode, WorkflowNodeRun, WorkflowRun, WorkflowStatus, WorkflowSubflowType},
};
use runinator_workflows::BranchPolicy;
use serde_json::{Map, Value};

use crate::{api::WorkflowSchedulerApi, context::*};

pub async fn process_task_node(
    _broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if let Some(node_run) = latest.filter(|run| run.status.is_terminal()) {
        match node_run.status {
            WorkflowStatus::Succeeded => {
                transition_from_node(
                    api,
                    workflow_run,
                    node,
                    node_run,
                    WorkflowStatus::Succeeded,
                    node_run.output_json.clone(),
                    None,
                    node_runs,
                )
                .await?;
            }
            WorkflowStatus::Failed | WorkflowStatus::TimedOut | WorkflowStatus::Canceled => {
                retry_or_transition(
                    api,
                    workflow_run,
                    node,
                    node_run,
                    node_run.status,
                    node_run.output_json.clone(),
                    node_run.message.clone(),
                    node_runs,
                )
                .await?;
            }
            _ => {}
        }
        return Ok(());
    }

    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Running) {
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

    let action = node.action.as_ref().ok_or_else(|| {
        Box::new(RuntimeError::new(
            "workflow.node.action_missing".into(),
            format!("Action node {} has no action configuration", node.id),
        )) as SendableError
    })?;
    let node_run = if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Queued)
    {
        node_run.clone()
    } else {
        api.create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
            .await?
    };
    let parameters = build_node_parameters(action, node, workflow_run, node_runs)?;
    let attempt = node_run.attempt + 1;
    let idempotency_scope = "workflow_action_node";
    let idempotency_key =
        workflow_task_idempotency_key(workflow_run.id, &node.id, node_run.id, attempt);
    if api
        .fetch_idempotency_key(idempotency_scope, &idempotency_key)
        .await?
        .is_none()
    {
        crate::iteration::enqueue_action_with_dedupe(
            api,
            workflow_run.id,
            &node_run,
            action,
            parameters.clone(),
            format!("workflow-node-run:{}", node_run.id),
        )
        .await?;
        api.put_idempotency_key(
            idempotency_scope,
            &idempotency_key,
            serde_json::json!({ "workflow_node_run_id": node_run.id }),
        )
        .await?;
    }

    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(attempt),
        Some(parameters),
        None,
        None,
        Some("action_started".into()),
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

pub(crate) fn workflow_task_idempotency_key(
    workflow_run_id: i64,
    node_id: &str,
    workflow_node_run_id: i64,
    attempt: i64,
) -> String {
    format!("{workflow_run_id}:{node_id}:{workflow_node_run_id}:{attempt}")
}

pub async fn process_wait_node(
    api: &dyn WorkflowSchedulerApi,
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
    api: &dyn WorkflowSchedulerApi,
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

pub async fn process_switch_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    let params = runinator_workflows::parse_switch_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let context = runtime_context(workflow_run, node_runs);
    let target = runinator_workflows::evaluate_switch(&params, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let output = serde_json::json!({ "target": target });
    api.update_workflow_node_run(
        node_run.id,
        if target.is_some() {
            WorkflowStatus::Succeeded
        } else {
            WorkflowStatus::Blocked
        },
        Some(node_run.attempt + 1),
        None,
        Some(output),
        None,
        Some("switch_evaluated".into()),
        None,
    )
    .await?;
    if let Some(target) = target {
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(target),
            None,
            None,
        )
        .await
    } else {
        transition_from_node(
            api,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Blocked,
            None,
            Some("Switch did not match a target".into()),
            node_runs,
        )
        .await
    }
}

pub async fn process_config_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    let context = runtime_context(workflow_run, node_runs);
    let resolved = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;

    let new_name = resolved.get("name").and_then(|value| match value {
        Value::Null => None,
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        other => Some(other.to_string()),
    });
    let metadata_patch = resolved.get("metadata").cloned();

    if new_name.is_some() {
        api.set_workflow_run_name(workflow_run.id, new_name.clone())
            .await?;
    }

    let mut summary_state = serde_json::Map::new();
    if let Some(ref name) = new_name {
        summary_state.insert("name".into(), Value::String(name.clone()));
    }
    if let Some(metadata) = metadata_patch.as_ref() {
        summary_state.insert("metadata".into(), metadata.clone());
    }

    // merge metadata into the run's state.run_metadata bag.
    if let Some(metadata) = metadata_patch {
        let mut state = workflow_run.state.clone();
        if !state.is_object() {
            state = Value::Object(Default::default());
        }
        let merged_metadata = if let Some(existing) = state.get("run_metadata").cloned() {
            merge_json(existing, metadata)
        } else {
            metadata
        };
        if let Value::Object(map) = &mut state {
            map.insert("run_metadata".into(), merged_metadata);
        }
        api.update_workflow_run(
            workflow_run.id,
            workflow_run.status,
            workflow_run.active_node_id.clone(),
            Some(state),
            None,
        )
        .await?;
    }

    let output = Value::Object(summary_state);
    transition_from_node(
        api,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output),
        Some("config_applied".into()),
        node_runs,
    )
    .await
}

fn merge_json(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Object(mut left), Value::Object(right)) => {
            for (key, value) in right {
                let existing = left.remove(&key);
                let merged = match existing {
                    Some(prev) => merge_json(prev, value),
                    None => value,
                };
                left.insert(key, merged);
            }
            Value::Object(left)
        }
        (_, right) => right,
    }
}

pub async fn process_emit_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    let params = runinator_workflows::parse_emit_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let context = runtime_context(workflow_run, node_runs);
    let data = runinator_workflows::resolve_value_refs(&params.data, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let output = serde_json::json!({
        "event_type": params.event_type,
        "data": data
    });
    transition_from_node(
        api,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output),
        Some("emit_recorded".into()),
        node_runs,
    )
    .await
}

pub async fn process_approval_node(
    api: &dyn WorkflowSchedulerApi,
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
    api: &dyn WorkflowSchedulerApi,
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
    let node_run = if let Some(latest) = latest.filter(|run| run.status == WorkflowStatus::Running)
    {
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
        let return_to = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string())
            .unwrap_or_else(|| node.id.clone());
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

pub async fn process_parallel_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
) -> Result<(), SendableError> {
    if latest.is_some() {
        return Ok(());
    }
    let params = runinator_workflows::parse_parallel_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let Some(first) = params.branches.first().cloned() else {
        return block_node(api, workflow_run, node, "Parallel node has no branches").await;
    };
    let remaining = params
        .branches
        .iter()
        .skip(1)
        .map(|branch| branch.as_str().to_string())
        .collect::<Vec<_>>();
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await?;
    let output = serde_json::json!({
        "branches": params.branches.iter().map(|branch| branch.as_str()).collect::<Vec<_>>()
    });
    let state = merge_state(
        &workflow_run.state,
        "parallel",
        serde_json::json!({
            "node_id": node.id,
            "remaining": remaining,
        }),
    );
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        Some(node_run.attempt + 1),
        None,
        Some(output),
        None,
        Some("parallel_started".into()),
        None,
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(first.into_string()),
        Some(state),
        None,
    )
    .await
}

pub async fn process_join_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_join_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    if join_satisfied(
        &params
            .wait_for
            .iter()
            .map(|target| target.as_str().to_string())
            .collect::<Vec<_>>(),
        params.mode,
        node_runs,
    ) {
        let node_run = ensure_node_run(api, workflow_run, node, latest).await?;
        let output = serde_json::json!({
            "wait_for": params.wait_for.iter().map(|target| target.as_str()).collect::<Vec<_>>(),
            "mode": branch_policy_name(params.mode)
        });
        return transition_from_node(
            api,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output),
            Some("join_satisfied".into()),
            node_runs,
        )
        .await;
    }
    if let Some(next_branch) = pop_state_queue(&workflow_run.state, "parallel", "remaining") {
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(next_branch.target),
            Some(next_branch.state),
            Some("join_waiting_for_parallel_branch".into()),
        )
        .await?;
        return Ok(());
    }
    let node_run = ensure_node_run(api, workflow_run, node, latest).await?;
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        None,
        Some("join_waiting".into()),
        None,
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        None,
        None,
    )
    .await
}

pub async fn process_map_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_map_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let mut frame = workflow_run.state.get("map").cloned();
    let node_run = ensure_node_run(api, workflow_run, node, latest).await?;
    if frame
        .as_ref()
        .and_then(|frame| frame.get("node_id"))
        .and_then(Value::as_str)
        != Some(node.id.as_str())
    {
        let context = runtime_context(workflow_run, node_runs);
        let items = runinator_workflows::resolve_value_refs(&params.items, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let items = items.as_array().cloned().unwrap_or_default();
        frame = Some(serde_json::json!({
            "node_id": node.id,
            "target": params.target.as_str(),
            "items": items,
            "index": 0,
            "outputs": [],
            "concurrency": params.concurrency.unwrap_or(1)
        }));
    } else {
        if let Some(status) = latest_status(params.target.as_str(), node_runs) {
            if status != WorkflowStatus::Succeeded {
                return transition_from_node(
                    api,
                    workflow_run,
                    node,
                    &node_run,
                    status,
                    None,
                    Some("map_item_failed".into()),
                    node_runs,
                )
                .await;
            }
        }
        frame = append_completed_map_item(frame, params.target.as_str(), node_runs);
    }
    let Some(frame_value) = frame else {
        return block_node(
            api,
            workflow_run,
            node,
            "Map state could not be initialized",
        )
        .await;
    };
    let items = frame_value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let index = frame_value
        .get("index")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if index >= items.len() as i64 {
        let output = serde_json::json!({
            "count": items.len(),
            "outputs": frame_value.get("outputs").cloned().unwrap_or_else(|| serde_json::json!([]))
        });
        return transition_from_node(
            api,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output),
            Some("map_exhausted".into()),
            node_runs,
        )
        .await;
    }
    let mut next_frame = frame_value;
    if let Some(object) = next_frame.as_object_mut() {
        object.insert("item".into(), items[index as usize].clone());
    }
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(next_frame.clone()),
        Some("map_iteration".into()),
        None,
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(params.target.into_string()),
        Some(merge_state(&workflow_run.state, "map", next_frame)),
        None,
    )
    .await
}

pub async fn process_race_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_race_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_run = ensure_node_run(api, workflow_run, node, latest).await?;
    let branches = params
        .branches
        .iter()
        .map(|branch| branch.as_str().to_string())
        .collect::<Vec<_>>();
    if let Some(winner) = race_winner(&branches, params.winner, node_runs) {
        let output = serde_json::json!({ "winner": winner });
        return transition_from_node(
            api,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output),
            Some("race_won".into()),
            node_runs,
        )
        .await;
    }
    let next = if workflow_run
        .state
        .get("race")
        .and_then(|frame| frame.get("node_id"))
        .and_then(Value::as_str)
        == Some(node.id.as_str())
    {
        pop_state_queue(&workflow_run.state, "race", "remaining")
    } else {
        let remaining = branches.iter().skip(1).cloned().collect::<Vec<_>>();
        Some(QueuedState {
            target: branches[0].clone(),
            state: merge_state(
                &workflow_run.state,
                "race",
                serde_json::json!({
                    "node_id": node.id,
                    "remaining": remaining,
                }),
            ),
        })
    };
    if let Some(next) = next {
        api.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Running,
            Some(node_run.attempt + 1),
            None,
            None,
            None,
            Some("race_branch_started".into()),
            None,
        )
        .await?;
        return api
            .update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Running,
                Some(next.target),
                Some(next.state),
                None,
            )
            .await;
    }
    transition_from_node(
        api,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Failed,
        None,
        Some("Race completed without a winning branch".into()),
        node_runs,
    )
    .await
}

pub async fn process_try_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_try_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_run = ensure_node_run(api, workflow_run, node, latest).await?;
    let frame = workflow_run.state.get("try").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "node_id": node.id,
            "phase": "body"
        })
    });
    let phase = frame.get("phase").and_then(Value::as_str).unwrap_or("body");
    if latest.is_none() {
        return start_try_phase(
            api,
            workflow_run,
            &node_run,
            node,
            params.body.as_str(),
            "body",
            None,
        )
        .await;
    }
    match phase {
        "body" => {
            let Some(status) = latest_status(params.body.as_str(), node_runs) else {
                return Ok(());
            };
            if status == WorkflowStatus::Succeeded {
                if let Some(finally) = params.finally {
                    return start_try_phase(
                        api,
                        workflow_run,
                        &node_run,
                        node,
                        finally.as_str(),
                        "finally",
                        Some(status),
                    )
                    .await;
                }
                return transition_from_node(
                    api,
                    workflow_run,
                    node,
                    &node_run,
                    status,
                    None,
                    Some("try_body_succeeded".into()),
                    node_runs,
                )
                .await;
            }
            if let Some(catch) = params.catch {
                return start_try_phase(
                    api,
                    workflow_run,
                    &node_run,
                    node,
                    catch.as_str(),
                    "catch",
                    Some(status),
                )
                .await;
            }
            if let Some(finally) = params.finally {
                return start_try_phase(
                    api,
                    workflow_run,
                    &node_run,
                    node,
                    finally.as_str(),
                    "finally",
                    Some(status),
                )
                .await;
            }
            transition_from_node(
                api,
                workflow_run,
                node,
                &node_run,
                status,
                None,
                Some("try_body_failed".into()),
                node_runs,
            )
            .await
        }
        "catch" => {
            let Some(status) = params
                .catch
                .as_ref()
                .and_then(|catch| latest_status(catch.as_str(), node_runs))
            else {
                return Ok(());
            };
            if let Some(finally) = params.finally {
                return start_try_phase(
                    api,
                    workflow_run,
                    &node_run,
                    node,
                    finally.as_str(),
                    "finally",
                    Some(status),
                )
                .await;
            }
            transition_from_node(
                api,
                workflow_run,
                node,
                &node_run,
                status,
                None,
                Some("try_catch_completed".into()),
                node_runs,
            )
            .await
        }
        "finally" => {
            let Some(finally) = params.finally.as_ref().map(|target| target.as_str()) else {
                return Ok(());
            };
            if latest_status(finally, node_runs).is_none() {
                return Ok(());
            }
            let status = frame
                .get("pending_status")
                .and_then(Value::as_str)
                .and_then(parse_workflow_status)
                .unwrap_or(WorkflowStatus::Succeeded);
            transition_from_node(
                api,
                workflow_run,
                node,
                &node_run,
                status,
                None,
                Some("try_finally_completed".into()),
                node_runs,
            )
            .await
        }
        _ => block_node(api, workflow_run, node, "Try node has invalid phase").await,
    }
}

pub async fn process_subflow_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if let Some(node_run) = latest {
        if let Some(subflow_run_id) = node_run.state.get("subflow_run_id").and_then(Value::as_i64) {
            if node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
                return transition_from_node(
                    api,
                    workflow_run,
                    node,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(node_run.state.clone()),
                    Some("subflow_linked".into()),
                    node_runs,
                )
                .await;
            }

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

    let subflow_id = resolve_subflow_id(api, node).await?;
    let context = runtime_context(workflow_run, node_runs);
    let parameters = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let run_name = resolve_optional_string(node.subflow.run_name.as_ref(), &context)?;
    let (subflow_run, reused) = if node.subflow.reuse_open_run {
        if let Some(name) = run_name.as_deref() {
            if let Some(existing) = api
                .fetch_workflow_runs_by_name(name, true)
                .await?
                .into_iter()
                .next()
            {
                (existing, true)
            } else {
                (
                    create_subflow_run(api, subflow_id, parameters.clone(), run_name.clone())
                        .await?,
                    false,
                )
            }
        } else {
            (
                create_subflow_run(api, subflow_id, parameters.clone(), None).await?,
                false,
            )
        }
    } else {
        (
            create_subflow_run(api, subflow_id, parameters.clone(), run_name.clone()).await?,
            false,
        )
    };
    let node_run = api
        .create_workflow_node_run(workflow_run.id, &node.id, parameters)
        .await?;
    let state = serde_json::json!({
        "subflow_run_id": subflow_run.id,
        "subflow_workflow_id": subflow_run.workflow_id,
        "run_name": run_name,
        "reused": reused
    });
    if node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
        api.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Succeeded,
            Some(node_run.attempt + 1),
            None,
            Some(state.clone()),
            Some(state.clone()),
            Some(if reused {
                "subflow_reused".into()
            } else {
                "subflow_started".into()
            }),
            None,
        )
        .await?;
        return transition_from_node(
            api,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(state.clone()),
            Some("subflow_linked".into()),
            node_runs,
        )
        .await;
    }

    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
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

async fn resolve_subflow_id(
    api: &dyn WorkflowSchedulerApi,
    node: &WorkflowNode,
) -> Result<i64, SendableError> {
    if let Some(subflow_id) = node.subflow_id {
        return Ok(subflow_id);
    }

    if let Some(workflow_name) = node.subflow.workflow_name.as_deref() {
        let workflow_name = workflow_name.trim();
        if !workflow_name.is_empty() {
            let workflow = api.fetch_workflow_by_name(workflow_name).await?;
            if let Some(id) = workflow.id {
                return Ok(id);
            }
            return Err(Box::new(RuntimeError::new(
                "workflow.subflow.missing_id".into(),
                format!("Subflow workflow {workflow_name} has no id"),
            )));
        }
    }

    Err(Box::new(RuntimeError::new(
        "workflow.subflow.target_missing".into(),
        format!("Subflow node {} is missing a target", node.id),
    )))
}

async fn create_subflow_run(
    api: &dyn WorkflowSchedulerApi,
    workflow_id: i64,
    parameters: Value,
    run_name: Option<String>,
) -> Result<WorkflowRun, SendableError> {
    match run_name {
        Some(name) => {
            api.create_named_workflow_run(workflow_id, parameters, name)
                .await
        }
        None => api.create_workflow_run(workflow_id, parameters).await,
    }
}

fn resolve_optional_string(
    value: Option<&Value>,
    context: &Value,
) -> Result<Option<String>, SendableError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let resolved = runinator_workflows::resolve_value_refs(value, context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let name = match resolved {
        Value::Null => None,
        Value::String(value) => Some(value.trim().to_string()).filter(|value| !value.is_empty()),
        other => Some(other.to_string()),
    };
    Ok(name)
}

pub async fn block_node(
    api: &dyn WorkflowSchedulerApi,
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

struct QueuedState {
    target: String,
    state: Value,
}

async fn ensure_node_run(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
) -> Result<WorkflowNodeRun, SendableError> {
    if let Some(latest) = latest {
        return Ok(latest.clone());
    }
    api.create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
        .await
}

fn merge_state(base: &Value, key: &str, value: Value) -> Value {
    let mut object = base.as_object().cloned().unwrap_or_default();
    object.insert(key.into(), value);
    Value::Object(object)
}

fn pop_state_queue(state: &Value, frame_key: &str, queue_key: &str) -> Option<QueuedState> {
    let mut root = state.as_object().cloned().unwrap_or_default();
    let mut frame = root.get(frame_key)?.as_object()?.clone();
    let mut remaining = frame.get(queue_key)?.as_array()?.clone();
    if remaining.is_empty() {
        return None;
    }
    let target = remaining.remove(0).as_str()?.to_string();
    frame.insert(queue_key.into(), Value::Array(remaining));
    root.insert(frame_key.into(), Value::Object(frame));
    Some(QueuedState {
        target,
        state: Value::Object(root),
    })
}

fn join_satisfied(wait_for: &[String], mode: BranchPolicy, node_runs: &[WorkflowNodeRun]) -> bool {
    match mode {
        BranchPolicy::All => wait_for
            .iter()
            .all(|node_id| latest_status(node_id, node_runs) == Some(WorkflowStatus::Succeeded)),
        BranchPolicy::Any | BranchPolicy::FirstSuccess => wait_for
            .iter()
            .any(|node_id| latest_status(node_id, node_runs) == Some(WorkflowStatus::Succeeded)),
    }
}

fn race_winner(
    branches: &[String],
    winner: BranchPolicy,
    node_runs: &[WorkflowNodeRun],
) -> Option<String> {
    match winner {
        BranchPolicy::All => {
            if branches
                .iter()
                .all(|node_id| latest_status(node_id, node_runs) == Some(WorkflowStatus::Succeeded))
            {
                branches.last().cloned()
            } else {
                None
            }
        }
        BranchPolicy::Any | BranchPolicy::FirstSuccess => branches
            .iter()
            .find(|node_id| latest_status(node_id, node_runs) == Some(WorkflowStatus::Succeeded))
            .cloned(),
    }
}

fn latest_status(node_id: &str, node_runs: &[WorkflowNodeRun]) -> Option<WorkflowStatus> {
    latest_node_run(node_runs, node_id).map(|run| run.status)
}

fn append_completed_map_item(
    frame: Option<Value>,
    target: &str,
    node_runs: &[WorkflowNodeRun],
) -> Option<Value> {
    let mut frame = frame?;
    let latest = latest_node_run(node_runs, target)?;
    if latest.status != WorkflowStatus::Succeeded {
        return Some(frame);
    }
    let object = frame.as_object_mut()?;
    let index = object
        .get("index")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let outputs = object
        .entry("outputs")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(outputs) = outputs.as_array_mut() else {
        return Some(Value::Object(object.clone()));
    };
    if outputs.len() as i64 <= index {
        outputs.push(latest.output_json.clone().unwrap_or(Value::Null));
        object.insert("index".into(), Value::from(index + 1));
    }
    Some(frame)
}

async fn start_try_phase(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node_run: &WorkflowNodeRun,
    node: &WorkflowNode,
    target: &str,
    phase: &str,
    pending_status: Option<WorkflowStatus>,
) -> Result<(), SendableError> {
    let mut frame = Map::new();
    frame.insert("node_id".into(), Value::String(node.id.clone()));
    frame.insert("phase".into(), Value::String(phase.into()));
    if let Some(status) = pending_status {
        frame.insert(
            "pending_status".into(),
            Value::String(status.as_str().into()),
        );
    }
    let state = merge_state(&workflow_run.state, "try", Value::Object(frame.clone()));
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(Value::Object(frame)),
        Some(format!("try_{phase}_started")),
        None,
    )
    .await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(target.into()),
        Some(state),
        None,
    )
    .await
}

fn parse_workflow_status(value: &str) -> Option<WorkflowStatus> {
    match value {
        "queued" => Some(WorkflowStatus::Queued),
        "running" => Some(WorkflowStatus::Running),
        "paused" => Some(WorkflowStatus::Paused),
        "waiting" => Some(WorkflowStatus::Waiting),
        "approval_required" => Some(WorkflowStatus::ApprovalRequired),
        "blocked" => Some(WorkflowStatus::Blocked),
        "succeeded" => Some(WorkflowStatus::Succeeded),
        "failed" => Some(WorkflowStatus::Failed),
        "timed_out" => Some(WorkflowStatus::TimedOut),
        "canceled" => Some(WorkflowStatus::Canceled),
        _ => None,
    }
}

fn branch_policy_name(policy: BranchPolicy) -> &'static str {
    match policy {
        BranchPolicy::All => "all",
        BranchPolicy::Any => "any",
        BranchPolicy::FirstSuccess => "first_success",
    }
}

pub async fn retry_or_transition(
    api: &dyn WorkflowSchedulerApi,
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
    api: &dyn WorkflowSchedulerApi,
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
