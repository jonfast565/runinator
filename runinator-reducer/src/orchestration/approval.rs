use super::context::is_reentry_stale;
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;

pub(super) struct ApprovalHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for ApprovalHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        // a loop body re-entering this node sees the prior iteration's resolved run; treat it as a
        // fresh visit so a new approval is requested instead of transitioning from the stale run.
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));
        if let Some(node_run) = latest {
            if node_run.status == WorkflowStatus::ApprovalRequired
                && timed_out_since_created(ctx.timing(), node_run)
            {
                return super::handler::complete(
                    time_out(ctx, node_run, "Approval timed out").await,
                );
            }
            if node_run.status == WorkflowStatus::Succeeded {
                transition_from_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    node_run.output_json.clone(),
                    Some("approval_resolved".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            return Ok(ReadyNodeDisposition::Complete);
        }
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
        let params = runinator_workflows::parse_approval_parameters(ctx.node);
        let record = ApprovalRecord {
            workflow_run_id: ctx.workflow_run.id,
            node_id: ctx.node.id.clone(),
            approval_type: params.approval_type,
            prompt: params.prompt,
            status: "pending".into(),
            provider: "runinator".into(),
            resource_type: "approval_request".into(),
            external_id: format!("workflow:{}:node:{}", ctx.workflow_run.id, ctx.node.id),
            metadata: params.metadata,
        };
        let approval = ctx
            .db
            .create_automation_record("approval_requests".into(), record.to_wire_value()?)
            .await?;
        let approval_state = ApprovalState {
            approval: ctx.node.parameters.clone().into(),
            approval_id: approval
                .get("id")
                .and_then(Value::as_str)
                .and_then(|raw| raw.parse::<Uuid>().ok()),
        };
        ctx.db
            .update_workflow_node_run(
                node_run.id,
                WorkflowStatus::ApprovalRequired,
                Some(node_run.attempt + 1),
                None,
                None,
                Some(approval_state.to_wire_value()?),
                Some(WorkflowStatus::ApprovalRequired.as_str().into()),
                None,
            )
            .await?;
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::ApprovalRequired,
                Some(ctx.node.id.clone()),
                None,
                None,
            )
            .await?;
        super::handler::complete(arm_node_timeout(ctx).await)
    }
}
