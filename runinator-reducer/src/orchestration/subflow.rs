use super::context::{is_reentry_stale, runtime_context};
use super::transitions::{arm_node_timeout, timed_out_since_created, transition_from_node};
use super::*;
use uuid::Uuid;

pub(super) struct SubflowHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for SubflowHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        // a loop body re-entering this node sees the prior iteration's linked subflow; treat it as a
        // fresh visit so a new child run is spawned instead of re-linking the stale one.
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));
        if let Some(node_run) = latest
            && let Ok(subflow_state) = SubflowState::from_wire_value(&node_run.state)
        {
            let subflow_run_id = subflow_state.subflow_run_id;
            if ctx.node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
                transition_from_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(node_run.state.clone()),
                    Some("subflow_linked".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            let Some(subflow_run) = ctx.db.fetch_workflow_run(subflow_run_id).await? else {
                return Err(crate::errors::SUBFLOW_RUN_MISSING.error(subflow_run_id));
            };
            match subflow_run.status {
                WorkflowStatus::Succeeded => {
                    let output = SubflowOutcome {
                        subflow_run_id,
                        status: subflow_run.status.as_str().to_string(),
                        state: Some(subflow_run.execution_state.to_state()),
                        parameters: Some(subflow_run.parameters),
                    };
                    transition_from_node(
                        ctx,
                        node_run,
                        WorkflowStatus::Succeeded,
                        Some(output.to_wire_value()?),
                        Some("subflow_succeeded".into()),
                    )
                    .await?;
                    return Ok(ReadyNodeDisposition::Complete);
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
                        ctx,
                        node_run,
                        WorkflowStatus::Failed,
                        Some(output.to_wire_value()?),
                        subflow_run
                            .message
                            .or(Some("Subflow did not succeed".into())),
                    )
                    .await?;
                    return Ok(ReadyNodeDisposition::Complete);
                }
                other => {
                    // wait-type subflow still in flight; fail fast once it overruns the timeout.
                    if timed_out_since_created(ctx.timing(), node_run) {
                        let timeout = ctx.node.timeout_seconds.unwrap_or_default();
                        let output = SubflowOutcome {
                            subflow_run_id,
                            status: other.as_str().to_string(),
                            state: None,
                            parameters: None,
                        };
                        transition_from_node(
                            ctx,
                            node_run,
                            WorkflowStatus::TimedOut,
                            Some(output.to_wire_value()?),
                            Some(format!(
                                "Subflow run {subflow_run_id} timed out after {timeout}s while {}",
                                other.as_str()
                            )),
                        )
                        .await?;
                        return Ok(ReadyNodeDisposition::Complete);
                    }
                    return Ok(ReadyNodeDisposition::Complete);
                }
            }
        }

        let subflow_id = resolve_subflow_id(ctx).await?;
        let context = runtime_context(ctx).await;
        let parameters = runinator_workflows::resolve_value_refs(&ctx.node.parameters, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let run_name = resolve_optional_string(ctx.node.subflow.run_name.as_ref(), &context)?;
        let (subflow_run, reused) = if ctx.node.subflow.reuse_open_run {
            match run_name.as_deref() {
                Some(name) => match ctx
                    .db
                    .fetch_workflow_runs_by_name(name.to_string(), true)
                    .await?
                    .into_iter()
                    .next()
                {
                    Some(existing) => (existing, true),
                    None => (
                        create_subflow_run(ctx, subflow_id, parameters.clone(), run_name.clone())
                            .await?,
                        false,
                    ),
                },
                None => (
                    create_subflow_run(ctx, subflow_id, parameters.clone(), None).await?,
                    false,
                ),
            }
        } else {
            (
                create_subflow_run(ctx, subflow_id, parameters.clone(), run_name.clone()).await?,
                false,
            )
        };
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                parameters,
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let state = SubflowState {
            subflow_run_id: subflow_run.id,
            subflow_workflow_id: subflow_run.workflow_id,
            run_name,
            reused,
        }
        .to_wire_value()?;
        if ctx.node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
            ctx.db
                .update_workflow_node_run(
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
                ctx,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(state.clone()),
                Some("subflow_linked".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }

        ctx.db
            .update_workflow_node_run(
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
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::Waiting,
                Some(ctx.node.id.clone()),
                None,
                None,
            )
            .await?;
        super::handler::complete(arm_node_timeout(ctx).await)
    }
}

/// resolve a subflow node's target workflow id from an explicit id or workflow name.
pub(super) async fn resolve_subflow_id<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
) -> Result<Uuid, SendableError> {
    if let Some(subflow_id) = ctx.node.subflow_id {
        return Ok(subflow_id);
    }
    if let Some(workflow_name) = ctx.node.subflow.workflow_name.as_deref() {
        let workflow_name = workflow_name.trim();
        if !workflow_name.is_empty() {
            let workflow = ctx
                .db
                .fetch_workflow_by_name(workflow_name.to_string())
                .await?
                .ok_or_else(|| crate::errors::SUBFLOW_MISSING.error(workflow_name))?;
            if let Some(id) = workflow.id {
                return Ok(id);
            }
            return Err(crate::errors::SUBFLOW_MISSING_ID.error(workflow_name));
        }
    }
    Err(crate::errors::SUBFLOW_TARGET_MISSING.error(&ctx.node.id))
}

/// create a child workflow run, stamp its parent linkage into state, and enqueue its start node so
/// the reducer drives it. the parent linkage lets a terminal child wake the waiting parent node.
pub(super) async fn create_subflow_run<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    workflow_id: Uuid,
    parameters: Value,
    run_name: Option<String>,
) -> Result<WorkflowRun, SendableError> {
    let snapshot = ctx
        .db
        .fetch_workflow(workflow_id)
        .await?
        .ok_or_else(|| crate::errors::WORKFLOW_NOT_FOUND.error(workflow_id))?;
    let state = runinator_models::json!({
        "control": { "pause_requested": false },
        "subflow_parent": { "run_id": ctx.workflow_run.id, "node_id": ctx.node.id }
    });
    let run = ctx
        .db
        .create_workflow_run(
            workflow_id,
            snapshot,
            parameters,
            state,
            run_name,
            runinator_models::replicas::WorkflowRunProvenance {
                source_kind: Some(runinator_models::replicas::TriggerSourceKind::Subflow),
                actor_type: Some(runinator_models::replicas::TriggerActorType::System),
                actor_replica_id: None,
                actor_display_name: Some("subflow".into()),
                request_host: None,
                request_ip: None,
                metadata: runinator_models::json!({
                    "parent_run_id": ctx.workflow_run.id,
                    "parent_node_id": ctx.node.id,
                }),
            },
        )
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
        ctx.db.enqueue_ready_node(event, start, Utc::now()).await?;
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
