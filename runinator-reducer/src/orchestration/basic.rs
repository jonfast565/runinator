use super::context::runtime_context;
use super::transitions::{ensure_completed_node_run, ensure_node_run, transition_from_node};
use super::*;

pub(super) struct ConfigHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for ConfigHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let context = runtime_context(ctx).await;
        let resolved = runinator_workflows::resolve_value_refs(&ctx.node.parameters, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let new_name = resolved.get("name").and_then(|value| match value {
            Value::Null => None,
            Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
            other => Some(other.to_string()),
        });
        if new_name.is_some() {
            ctx.db
                .set_workflow_run_name(ctx.workflow_run.id, new_name.clone())
                .await?;
        }
        let summary = ConfigSummary {
            name: new_name,
            metadata: resolved.get("metadata").cloned(),
        };
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(summary.to_wire_value()?),
            Some("config_applied".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

pub(super) async fn skip_node<T: ReducerStore>(
    ctx: &super::handler::NodeStepContext<'_, T>,
) -> Result<(), SendableError> {
    let node_run = ensure_node_run(
        ctx,
        super::context::most_recently_finished_node_run(ctx.node_runs),
    )
    .await?;
    let output = SkippedOutput {
        skipped: true,
        node_id: ctx.node.id.clone(),
    };
    transition_from_node(
        ctx,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some(format!("Node {} skipped", ctx.node.id)),
    )
    .await?;
    Ok(())
}

pub(super) struct StartHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for StartHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ensure_node_run(
            ctx,
            super::context::most_recently_finished_node_run(ctx.node_runs),
        )
        .await?;
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Succeeded,
            None,
            Some("start_reached".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

/// process an `interrupt` node: the entry of a handler region.
///
/// a no-op like `start`, for the same reason — it marks where a thread of control begins rather
/// than doing work. the node run it records is what puts the region's entry on the run timeline,
/// and `interrupt.*` already resolves here because the frame is written when the handler cursor is
/// created, before the cursor is ever driven.
pub(super) struct InterruptHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for InterruptHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ensure_node_run(
            ctx,
            super::context::most_recently_finished_node_run(ctx.node_runs),
        )
        .await?;
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Succeeded,
            None,
            Some("interrupt_entered".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

pub(super) struct EndHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for EndHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        ensure_completed_node_run(ctx, "end_reached").await?;
        // reaching `end` finishes *this* thread of control, not necessarily the run. settling through
        // `advance_cursor` is what applies that rule: the run takes `Succeeded` only once the last
        // cursor retires, so one branch of a fan-out arriving here cannot end the run under its
        // still-running siblings.
        run_state::advance_cursor(
            ctx.db,
            ctx.workflow_run.id,
            ctx.cursor.id,
            WorkflowStatus::Succeeded,
            run_state::CursorMove::Retire,
            None,
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

pub(super) struct ConditionHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for ConditionHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let context = runtime_context(ctx).await;
        let matched =
            runinator_workflows::evaluate_workflow_condition(&ctx.node.condition, &context)
                .map_err(|err| -> SendableError { Box::new(err) })?;
        let (status, reason) = if matched {
            (WorkflowStatus::Succeeded, "condition_matched")
        } else {
            (WorkflowStatus::Blocked, "condition_unmatched")
        };
        transition_from_node(ctx, &node_run, status, None, Some(reason.into())).await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

pub(super) struct SwitchHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for SwitchHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let params = runinator_workflows::parse_switch_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let context = runtime_context(ctx).await;
        let target = runinator_workflows::evaluate_switch(&params, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        super::handler::complete(
            finish_route(
                ctx,
                &node_run,
                target,
                "switch_evaluated",
                "Switch did not match a target",
            )
            .await,
        )
    }
}

pub(super) struct ToggleHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for ToggleHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let params = runinator_workflows::parse_toggle_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let context = runtime_context(ctx).await;
        // a toggle always yields a target, so it never blocks the run.
        let target = runinator_workflows::evaluate_toggle(&params, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        super::handler::complete(
            finish_route(
                ctx,
                &node_run,
                Some(target),
                "toggle_evaluated",
                "Toggle did not resolve a target",
            )
            .await,
        )
    }
}

pub(super) struct PercentageHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for PercentageHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let params = runinator_workflows::parse_percentage_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let context = runtime_context(ctx).await;
        let target = runinator_workflows::evaluate_percentage(&params, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        super::handler::complete(
            finish_route(
                ctx,
                &node_run,
                target,
                "percentage_evaluated",
                "Percentage did not match a bucket",
            )
            .await,
        )
    }
}

// record the router node run's chosen target and route the run: `Some` drives the run to the target
// (Running), `None` blocks the node and follows its failure transition. shared by switch/toggle/percentage.
async fn finish_route<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    node_run: &WorkflowNodeRun,
    target: Option<String>,
    reason: &str,
    no_match: &str,
) -> Result<(), SendableError> {
    let output = SwitchOutput {
        target: target.clone(),
    }
    .to_wire_value()?;
    ctx.db
        .update_workflow_node_run(
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
            Some(reason.into()),
            None,
        )
        .await?;
    match target {
        // a router moves *this* thread of control. writing `active_node_id` instead would leave the
        // cursor on the router, and the drive would re-process it until the inline-step limit
        // blocked the run.
        Some(target) => {
            run_state::advance_cursor(
                ctx.db,
                ctx.workflow_run.id,
                ctx.cursor.id,
                WorkflowStatus::Running,
                run_state::CursorMove::To(target),
                None,
            )
            .await?;
        }
        None => {
            transition_from_node(
                ctx,
                node_run,
                WorkflowStatus::Blocked,
                None,
                Some(no_match.into()),
            )
            .await?;
        }
    }
    Ok(())
}

// --- rich control-flow nodes -------------------------------------------------
//
// the reducer lives here and calls `ReducerStore` directly. control-flow bookkeeping lives in
// named frames inside the typed `workflow_run.execution_state` aggregate.
// predicates that read sibling node-run history come from runinator-workflows.
