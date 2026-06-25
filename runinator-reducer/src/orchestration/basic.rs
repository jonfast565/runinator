use std::future::Future;

use super::context::runtime_context;
use super::handler::{NodeHandler, NodeHandlerContext};
use super::transitions::{ensure_completed_node_run, ensure_node_run, transition_from_node};
use super::*;

pub(super) async fn process_config_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let context = runtime_context(db, workflow_run, node_runs).await;
    let resolved = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let new_name = resolved.get("name").and_then(|value| match value {
        Value::Null => None,
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        other => Some(other.to_string()),
    });
    if new_name.is_some() {
        db.set_workflow_run_name(workflow_run.id, new_name.clone())
            .await?;
    }
    let summary = ConfigSummary {
        name: new_name,
        metadata: resolved.get("metadata").cloned(),
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(summary.to_wire_value()?),
        Some("config_applied".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) async fn process_skipped_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    let output = SkippedOutput {
        skipped: true,
        node_id: node.id.clone(),
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some(format!("Node {} skipped", node.id)),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) async fn process_start_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        None,
        Some("start_reached".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) async fn process_end_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
) -> Result<(), SendableError> {
    ensure_completed_node_run(db, workflow_run, node, latest, "end_reached").await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Succeeded,
        Some(node.id.clone()),
        None,
        None,
    )
    .await?;
    Ok(())
}

pub(super) async fn process_condition_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let context = runtime_context(db, workflow_run, node_runs).await;
    let matched = runinator_workflows::evaluate_condition(&node.condition, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let (status, reason) = if matched {
        (WorkflowStatus::Succeeded, "condition_matched")
    } else {
        (WorkflowStatus::Blocked, "condition_unmatched")
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        status,
        None,
        Some(reason.into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) async fn process_switch_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let params = runinator_workflows::parse_switch_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let context = runtime_context(db, workflow_run, node_runs).await;
    let target = runinator_workflows::evaluate_switch(&params, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let output = SwitchOutput {
        target: target.clone(),
    }
    .to_wire_value()?;
    db.update_workflow_node_run(
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
    match target {
        Some(target) => {
            db.update_workflow_run_status(
                workflow_run.id,
                WorkflowStatus::Running,
                Some(target),
                None,
                None,
            )
            .await?;
        }
        None => {
            transition_from_node(
                db,
                workflow_run,
                node,
                &node_run,
                WorkflowStatus::Blocked,
                None,
                Some("Switch did not match a target".into()),
                node_runs,
            )
            .await?;
        }
    }
    Ok(())
}

pub(super) struct StartHandler;
pub(super) struct EndHandler;
pub(super) struct ConditionHandler;
pub(super) struct SwitchHandler;
pub(super) struct ConfigHandler;

impl<T: DatabaseImpl> NodeHandler<T> for StartHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_start_node(
                ctx.db,
                ctx.workflow_run,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

impl<T: DatabaseImpl> NodeHandler<T> for EndHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_end_node(ctx.db, ctx.workflow_run, ctx.node, ctx.latest).await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

impl<T: DatabaseImpl> NodeHandler<T> for ConditionHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_condition_node(ctx.db, ctx.workflow_run, ctx.node, ctx.node_runs).await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

impl<T: DatabaseImpl> NodeHandler<T> for SwitchHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_switch_node(ctx.db, ctx.workflow_run, ctx.node, ctx.node_runs).await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

impl<T: DatabaseImpl> NodeHandler<T> for ConfigHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_config_node(ctx.db, ctx.workflow_run, ctx.node, ctx.node_runs).await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

// --- rich control-flow nodes -------------------------------------------------
//
// the reducer lives here and calls `DatabaseImpl` directly. control-flow bookkeeping lives in
// named frames inside `workflow_run.state` (the typed `WorkflowRunState` from runinator-models).
// predicates that read sibling node-run history come from runinator-workflows.
