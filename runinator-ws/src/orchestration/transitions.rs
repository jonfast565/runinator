use super::context::{runtime_context, set_step_output};
use super::*;

// --- shared db-direct reducer helpers -----------------------------------------

/// settle a node run, retrying while attempts remain, otherwise transitioning.
#[allow(clippy::too_many_arguments)]
pub(super) async fn retry_or_transition<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if node_run.attempt < node.retry.max_attempts {
        db.update_workflow_node_run(
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
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(node.id.clone()),
            None,
            None,
        )
        .await
    } else {
        transition_from_node(
            db,
            workflow_run,
            node,
            node_run,
            status,
            output_json,
            message,
            node_runs,
        )
        .await?;
        Ok(())
    }
}

/// time out the in-flight run with a node-specific message, retrying if attempts remain.
pub(super) async fn time_out<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    message: &str,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    retry_or_transition(
        db,
        workflow_run,
        node,
        node_run,
        WorkflowStatus::TimedOut,
        None,
        Some(message.into()),
        node_runs,
    )
    .await
}

/// create a node run and block the workflow with a message.
pub(super) async fn block_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    message: &str,
) -> Result<(), SendableError> {
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Blocked,
        Some(node_run.attempt + 1),
        None,
        None,
        None,
        Some(WorkflowStatus::Blocked.as_str().into()),
        Some(message.into()),
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Blocked,
        Some(node.id.clone()),
        None,
        Some(message.into()),
    )
    .await
}

/// advance a try node into a phase (body/catch/finally), recording the phase frame.
pub(super) async fn start_try_phase<T: DatabaseImpl>(
    db: &T,
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
    let mut run_state = WorkflowRunState::from_state(&workflow_run.state);
    run_state.try_frame = Some(frame.clone());
    let state = run_state.to_state();
    db.update_workflow_node_run(
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
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(target.into()),
        Some(state),
        None,
    )
    .await
}

/// true when the run started more than `node.timeout_seconds` ago.
pub(super) fn timed_out(node: &WorkflowNode, run: &WorkflowNodeRun) -> bool {
    let (Some(timeout), Some(started_at)) = (node.timeout_seconds, run.started_at) else {
        return false;
    };
    Utc::now() - started_at > chrono::Duration::seconds(timeout)
}

/// like `timed_out`, but measured from run creation (used by subflow waits).
pub(super) fn timed_out_since_created(node: &WorkflowNode, run: &WorkflowNodeRun) -> bool {
    let Some(timeout) = node.timeout_seconds else {
        return false;
    };
    Utc::now() - run.created_at > chrono::Duration::seconds(timeout)
}

/// enqueue a delayed self ready node at a node's timeout deadline. the event-driven ready queue does
/// not re-poll parked nodes, so a node that parks (approval/join/subflow) re-arms its own timeout so
/// the timeout check fires even when no external wake-up arrives.
pub(super) async fn arm_node_timeout<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    node: &WorkflowNode,
) -> Result<(), SendableError> {
    let Some(timeout) = node.timeout_seconds else {
        return Ok(());
    };
    let deadline = Utc::now() + chrono::Duration::seconds(timeout);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "node_timeout_rearm",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), deadline)
        .await?;
    Ok(())
}

/// when a child workflow run reaches a terminal state, wake the parent subflow node waiting on it.
/// the parent linkage is stamped into the child run's `state.subflow_parent` at creation.
pub(super) async fn maybe_wake_subflow_parent<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
) -> Result<(), SendableError> {
    if !run.status.is_terminal() {
        return Ok(());
    }
    let Some(parent) = run.state.get("subflow_parent") else {
        return Ok(());
    };
    let (Some(parent_run_id), Some(parent_node_id)) = (
        parent.get("run_id").and_then(Value::as_i64),
        parent.get("node_id").and_then(Value::as_str),
    ) else {
        return Ok(());
    };
    let event = NewOrchestrationEvent::new(
        parent_run_id,
        Some(parent_node_id.to_string()),
        "subflow_child_finished",
        runinator_models::json!({ "child_run_id": run.id, "status": run.status.as_str() }),
    );
    db.enqueue_ready_node(event, parent_node_id.to_string(), Utc::now())
        .await?;
    Ok(())
}

pub(super) async fn transition_from_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    node_runs: &[WorkflowNodeRun],
) -> Result<Option<String>, SendableError> {
    db.update_workflow_node_run(
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
    let mut context = runtime_context(db, workflow_run, node_runs).await;
    if let Some(output) = output_json {
        set_step_output(&mut context, &node.id, output);
    }
    let next = runinator_workflows::next_transition(node, status, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    match next {
        Some(next) => {
            db.update_workflow_run_status(
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
            db.update_workflow_run_status(
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
            db.update_workflow_run_status(
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

pub(super) async fn ensure_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
) -> Result<WorkflowNodeRun, SendableError> {
    if let Some(latest) = latest {
        return Ok(latest.clone());
    }
    db.create_workflow_node_run(
        workflow_run.id,
        node.id.clone(),
        node.parameters.clone().into(),
    )
    .await
}

pub(super) async fn ensure_completed_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    reason: &str,
) -> Result<(), SendableError> {
    if latest.is_some_and(|run| run.status == WorkflowStatus::Succeeded) {
        return Ok(());
    }
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    db.update_workflow_node_run(
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
