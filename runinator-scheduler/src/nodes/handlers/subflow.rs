// subflow node: launches (or reuses) a child workflow run and, for wait-type subflows, polls it to
// completion. fire-and-forget subflows settle as soon as the child is linked.

use async_trait::async_trait;
use runinator_comm::WireCodec;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    workflow_state::{SubflowOutcome, SubflowState},
    workflows::{WorkflowNode, WorkflowNodeKind, WorkflowRun, WorkflowStatus, WorkflowSubflowType},
};
use serde_json::Value;

use crate::api::WorkflowSchedulerApi;
use crate::context::runtime_context;
use crate::nodes::context::NodeContext;
use crate::nodes::handler::{NodeHandler, NodeOutcome};

pub struct SubflowHandler;

#[async_trait]
impl NodeHandler for SubflowHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Subflow
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        // fail fast: a subflow error would otherwise bubble up to the scheduler loop, be logged,
        // and retried on every tick while the run stays non-terminal, leaving the node stuck
        // running. surface it as a failed node so the workflow can follow its on_failure transition
        // or fail the run instead of looping forever.
        match run_subflow_node(ctx).await {
            Ok(outcome) => Ok(outcome),
            Err(err) => {
                let node_run = ctx.ensure_node_run().await?;
                ctx.transition(
                    &node_run,
                    WorkflowStatus::Failed,
                    None,
                    Some(format!("Subflow node {} failed: {err}", ctx.node.id)),
                )
                .await
            }
        }
    }
}

async fn run_subflow_node(ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
    if let Some(node_run) = ctx.latest {
        if let Ok(subflow_state) = SubflowState::from_wire_value(&node_run.state) {
            let subflow_run_id = subflow_state.subflow_run_id;
            if ctx.node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
                return ctx
                    .transition(
                        node_run,
                        WorkflowStatus::Succeeded,
                        Some(node_run.state.clone()),
                        Some("subflow_linked".into()),
                    )
                    .await;
            }

            let (subflow_run, _) = ctx.api.fetch_workflow_run(subflow_run_id).await?;
            match subflow_run.status {
                WorkflowStatus::Succeeded => {
                    let output = SubflowOutcome {
                        subflow_run_id,
                        status: subflow_run.status.as_str().to_string(),
                        state: Some(subflow_run.state),
                        parameters: Some(subflow_run.parameters),
                    };
                    return ctx
                        .transition(
                            node_run,
                            WorkflowStatus::Succeeded,
                            Some(output.to_wire_value()?),
                            Some("subflow_succeeded".into()),
                        )
                        .await;
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
                    return ctx
                        .transition(
                            node_run,
                            WorkflowStatus::Failed,
                            Some(output.to_wire_value()?),
                            subflow_run
                                .message
                                .or(Some("Subflow did not succeed".into())),
                        )
                        .await;
                }
                other => {
                    // wait-type subflow is still in flight; fail fast once it overruns the timeout.
                    if ctx.timed_out_since_created(node_run) {
                        let timeout = ctx.node.timeout_seconds.unwrap_or_default();
                        let output = SubflowOutcome {
                            subflow_run_id,
                            status: other.as_str().to_string(),
                            state: None,
                            parameters: None,
                        };
                        return ctx
                            .transition(
                                node_run,
                                WorkflowStatus::TimedOut,
                                Some(output.to_wire_value()?),
                                Some(format!(
                                    "Subflow run {subflow_run_id} timed out after {timeout}s while {}",
                                    other.as_str()
                                )),
                            )
                            .await;
                    }
                    return Ok(NodeOutcome::Pending);
                }
            }
        }
    }

    let subflow_id = resolve_subflow_id(ctx.api, ctx.node).await?;
    let context = runtime_context(ctx.workflow_run, ctx.node_runs);
    let parameters = runinator_workflows::resolve_value_refs(&ctx.node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let run_name = resolve_optional_string(ctx.node.subflow.run_name.as_ref(), &context)?;
    let (subflow_run, reused) = if ctx.node.subflow.reuse_open_run {
        if let Some(name) = run_name.as_deref() {
            if let Some(existing) = ctx
                .api
                .fetch_workflow_runs_by_name(name, true)
                .await?
                .into_iter()
                .next()
            {
                (existing, true)
            } else {
                (
                    create_subflow_run(ctx.api, subflow_id, parameters.clone(), run_name.clone())
                        .await?,
                    false,
                )
            }
        } else {
            (
                create_subflow_run(ctx.api, subflow_id, parameters.clone(), None).await?,
                false,
            )
        }
    } else {
        (
            create_subflow_run(ctx.api, subflow_id, parameters.clone(), run_name.clone()).await?,
            false,
        )
    };
    let node_run = ctx.create_node_run_with(parameters).await?;
    let state = SubflowState {
        subflow_run_id: subflow_run.id,
        subflow_workflow_id: subflow_run.workflow_id,
        run_name,
        reused,
    }
    .to_wire_value()?;
    if ctx.node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
        ctx.update_node_run(
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
        return ctx
            .transition(
                &node_run,
                WorkflowStatus::Succeeded,
                Some(state.clone()),
                Some("subflow_linked".into()),
            )
            .await;
    }

    ctx.update_node_run(
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
    ctx.update_run(
        WorkflowStatus::Waiting,
        Some(ctx.node.id.clone()),
        Some(state),
        None,
    )
    .await?;
    Ok(NodeOutcome::Started)
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
