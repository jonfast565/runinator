// low-level persistence steps shared by every node handler.
//
// these own the exact api call sequences for completing a node run, routing to the next node,
// retrying, and blocking. `NodeContext` wraps them so handlers express intent; nothing here makes
// node-type decisions. each returns the flow disposition so the context layer can report it to the
// lifecycle hooks without re-deriving it.

use runinator_comm::WireCodec;
use runinator_models::{
    errors::SendableError,
    workflow_state::TryFrame,
    workflows::{WorkflowNode, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;

use crate::nodes::run_state::RunState;
use crate::{
    api::WorkflowSchedulerApi,
    context::{runtime_context, set_step_output},
};

/// outcome of `retry_or_transition`: either the run was requeued for another attempt, or it settled
/// and the workflow advanced to `target` (none when the workflow completed).
pub enum RetryDisposition {
    Retried,
    Transitioned(Option<String>),
}

/// reuse the latest run for this node, or create a fresh one.
pub async fn ensure_node_run(
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

/// mark a node run succeeded without changing the active node. used by terminal markers (end/fail).
pub async fn ensure_completed_node_run(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    reason: &str,
) -> Result<(), SendableError> {
    if latest.is_some_and(|run| run.status == WorkflowStatus::Succeeded) {
        return Ok(());
    }
    let node_run = ensure_node_run(api, workflow_run, node, latest).await?;
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        Some(node_run.attempt + 1),
        None,
        None,
        None,
        Some(reason.into()),
        None,
    )
    .await
}

/// settle a node run and advance the workflow along the node's transitions. returns the target the
/// workflow moved to, or none when the workflow run reached a terminal state.
pub async fn transition_from_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    node_runs: &[WorkflowNodeRun],
) -> Result<Option<String>, SendableError> {
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
        set_step_output(&mut context, &node.id, output);
    }
    let next = runinator_workflows::next_transition(node, status, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    match next {
        Some(next) => {
            api.update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Running,
                Some(next.clone()),
                None,
                message,
            )
            .await?;
            Ok(Some(next))
        }
        None if status == WorkflowStatus::Succeeded => {
            api.update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Succeeded,
                Some(node.id.clone()),
                None,
                message,
            )
            .await?;
            Ok(None)
        }
        None => {
            api.update_workflow_run(
                workflow_run.id,
                status,
                Some(node.id.clone()),
                None,
                message,
            )
            .await?;
            Ok(None)
        }
    }
}

/// requeue the run for another attempt when retries remain, otherwise settle and transition.
pub async fn retry_or_transition(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    node_runs: &[WorkflowNodeRun],
) -> Result<RetryDisposition, SendableError> {
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
        .await?;
        Ok(RetryDisposition::Retried)
    } else {
        let target = transition_from_node(
            api,
            workflow_run,
            node,
            node_run,
            status,
            output_json,
            message,
            node_runs,
        )
        .await?;
        Ok(RetryDisposition::Transitioned(target))
    }
}

/// create a node run and block the workflow with a message.
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

/// advance a try node into a phase (body/catch/finally), recording the phase frame.
pub async fn start_try_phase(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node_run: &WorkflowNodeRun,
    node: &WorkflowNode,
    target: &str,
    phase: &str,
    pending_status: Option<WorkflowStatus>,
) -> Result<(), SendableError> {
    let frame = TryFrame {
        node_id: node.id.clone(),
        phase: phase.into(),
        pending_status,
    };
    let mut run_state = RunState::from_value(&workflow_run.state);
    run_state.set_try(frame.clone());
    let state = run_state.into_value()?;
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(frame.to_wire_value()?),
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

/// recursively merge json objects, with `right` winning on scalar conflicts.
pub fn merge_json(left: Value, right: Value) -> Value {
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
