use super::transitions::timed_out_since_created;
use super::transitions::transition_from_node;
use super::*;

pub(super) struct InputHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for InputHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let latest = ctx.latest.filter(|run| run.node_id == ctx.node.id);
        if let Some(node_run) = latest {
            if node_run.status == WorkflowStatus::InputRequired
                && timed_out_since_created(ctx.timing(), node_run)
            {
                return super::handler::complete(
                    transitions::time_out(ctx, node_run, "Input timed out").await,
                );
            }
            if node_run.status == WorkflowStatus::Succeeded {
                transition_from_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    node_run.output_json.clone(),
                    Some("input_resolved".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            if node_run.status == WorkflowStatus::InputRequired {
                return Ok(ReadyNodeDisposition::Complete);
            }
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
        let state = InputState {
            input: ctx.node.parameters.clone().into(),
            input_id: None,
        };
        ctx.db
            .update_workflow_node_run(
                node_run.id,
                WorkflowStatus::InputRequired,
                Some(node_run.attempt + 1),
                None,
                None,
                Some(state.to_wire_value()?),
                Some(WorkflowStatus::InputRequired.as_str().into()),
                Some("input_requested".into()),
            )
            .await?;
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::InputRequired,
                Some(ctx.node.id.clone()),
                Some(state.to_wire_value()?),
                Some("input_requested".into()),
            )
            .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
