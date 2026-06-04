use super::context::{is_reentry_stale, runtime_context};
use super::transitions::{arm_node_timeout, timed_out_since_created, transition_from_node};
use super::*;

pub(super) async fn process_subflow_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    // a loop body re-entering this node sees the prior iteration's linked subflow; treat it as a
    // fresh visit so a new child run is spawned instead of re-linking the stale one.
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));
    if let Some(node_run) = latest
        && let Ok(subflow_state) = SubflowState::from_wire_value(&node_run.state)
    {
        let subflow_run_id = subflow_state.subflow_run_id;
        if node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                Some(node_run.state.clone()),
                Some("subflow_linked".into()),
                node_runs,
            )
            .await?;
            return Ok(());
        }
        let Some(subflow_run) = db.fetch_workflow_run(subflow_run_id).await? else {
            return Err(Box::new(RuntimeError::new(
                "workflow.subflow.run_missing".into(),
                format!("Subflow run {subflow_run_id} not found"),
            )));
        };
        match subflow_run.status {
            WorkflowStatus::Succeeded => {
                let output = SubflowOutcome {
                    subflow_run_id,
                    status: subflow_run.status.as_str().to_string(),
                    state: Some(subflow_run.state),
                    parameters: Some(subflow_run.parameters),
                };
                transition_from_node(
                    db,
                    workflow_run,
                    node,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("subflow_succeeded".into()),
                    node_runs,
                )
                .await?;
                return Ok(());
            }
            WorkflowStatus::Failed
            | WorkflowStatus::TimedOut
            | WorkflowStatus::Canceled
            | WorkflowStatus::Blocked => {
                let output = SubflowOutcome {
                    subflow_run_id,
                    status: subflow_run.status.as_str().to_string(),
                    state: None,
                    parameters: None,
                };
                transition_from_node(
                    db,
                    workflow_run,
                    node,
                    node_run,
                    WorkflowStatus::Failed,
                    Some(output.to_wire_value()?),
                    subflow_run
                        .message
                        .or(Some("Subflow did not succeed".into())),
                    node_runs,
                )
                .await?;
                return Ok(());
            }
            other => {
                // wait-type subflow still in flight; fail fast once it overruns the timeout.
                if timed_out_since_created(node, node_run) {
                    let timeout = node.timeout_seconds.unwrap_or_default();
                    let output = SubflowOutcome {
                        subflow_run_id,
                        status: other.as_str().to_string(),
                        state: None,
                        parameters: None,
                    };
                    transition_from_node(
                        db,
                        workflow_run,
                        node,
                        node_run,
                        WorkflowStatus::TimedOut,
                        Some(output.to_wire_value()?),
                        Some(format!(
                            "Subflow run {subflow_run_id} timed out after {timeout}s while {}",
                            other.as_str()
                        )),
                        node_runs,
                    )
                    .await?;
                    return Ok(());
                }
                return Ok(());
            }
        }
    }

    let subflow_id = resolve_subflow_id(db, node).await?;
    let context = runtime_context(workflow_run, node_runs);
    let parameters = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let run_name = resolve_optional_string(node.subflow.run_name.as_ref(), &context)?;
    let (subflow_run, reused) = if node.subflow.reuse_open_run {
        match run_name.as_deref() {
            Some(name) => match db
                .fetch_workflow_runs_by_name(name.to_string(), true)
                .await?
                .into_iter()
                .next()
            {
                Some(existing) => (existing, true),
                None => (
                    create_subflow_run(
                        db,
                        subflow_id,
                        parameters.clone(),
                        run_name.clone(),
                        workflow_run.id,
                        &node.id,
                    )
                    .await?,
                    false,
                ),
            },
            None => (
                create_subflow_run(
                    db,
                    subflow_id,
                    parameters.clone(),
                    None,
                    workflow_run.id,
                    &node.id,
                )
                .await?,
                false,
            ),
        }
    } else {
        (
            create_subflow_run(
                db,
                subflow_id,
                parameters.clone(),
                run_name.clone(),
                workflow_run.id,
                &node.id,
            )
            .await?,
            false,
        )
    };
    let node_run = db
        .create_workflow_node_run(workflow_run.id, node.id.clone(), parameters)
        .await?;
    let state = SubflowState {
        subflow_run_id: subflow_run.id,
        subflow_workflow_id: subflow_run.workflow_id,
        run_name,
        reused,
    }
    .to_wire_value()?;
    if node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
        db.update_workflow_node_run(
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
        transition_from_node(
            db,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(state.clone()),
            Some("subflow_linked".into()),
            node_runs,
        )
        .await?;
        return Ok(());
    }

    db.update_workflow_node_run(
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
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        Some(state),
        None,
    )
    .await?;
    arm_node_timeout(db, workflow_run.id, node).await
}

/// resolve a subflow node's target workflow id from an explicit id or workflow name.
pub(super) async fn resolve_subflow_id<T: DatabaseImpl>(
    db: &T,
    node: &WorkflowNode,
) -> Result<i64, SendableError> {
    if let Some(subflow_id) = node.subflow_id {
        return Ok(subflow_id);
    }
    if let Some(workflow_name) = node.subflow.workflow_name.as_deref() {
        let workflow_name = workflow_name.trim();
        if !workflow_name.is_empty() {
            let workflow = db
                .fetch_workflow_by_name(workflow_name.to_string())
                .await?
                .ok_or_else(|| {
                    Box::new(RuntimeError::new(
                        "workflow.subflow.missing".into(),
                        format!("Subflow workflow {workflow_name} not found"),
                    )) as SendableError
                })?;
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

/// create a child workflow run, stamp its parent linkage into state, and enqueue its start node so
/// the reducer drives it. the parent linkage lets a terminal child wake the waiting parent node.
pub(super) async fn create_subflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
    parameters: Value,
    run_name: Option<String>,
    parent_run_id: i64,
    parent_node_id: &str,
) -> Result<WorkflowRun, SendableError> {
    let snapshot = db.fetch_workflow(workflow_id).await?.ok_or_else(|| {
        Box::new(RuntimeError::new(
            "workflow.not_found".into(),
            format!("Workflow {workflow_id} not found"),
        )) as SendableError
    })?;
    let state = runinator_models::json!({
        "control": { "pause_requested": false },
        "subflow_parent": { "run_id": parent_run_id, "node_id": parent_node_id }
    });
    let run = db
        .create_workflow_run(workflow_id, snapshot, parameters, state, run_name)
        .await?;
    if let Some(snapshot) = run.workflow_snapshot.as_ref() {
        let (start, _) = runinator_workflows::parse_nodes(snapshot)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let event = NewOrchestrationEvent::new(
            run.id,
            Some(start.clone()),
            "subflow_run_created",
            runinator_models::json!({ "workflow_id": run.workflow_id, "node_id": start }),
        );
        db.enqueue_ready_node(event, start, Utc::now()).await?;
    }
    Ok(run)
}

pub(super) fn resolve_optional_string(
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
