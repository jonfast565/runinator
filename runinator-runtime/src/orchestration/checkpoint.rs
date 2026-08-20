use super::transitions::transition_from_node;
use super::*;

const RECORD_TYPE: &str = "workflow_checkpoint";

/// parse the checkpoint name from a node's parameters. falls back to the node id.
pub(super) fn parse_checkpoint_name(params: &Value, node_id: &str) -> String {
    params
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(node_id)
        .to_string()
}

/// process a checkpoint node: snapshot the current run state and active_node_id into an
/// automation_record row so a control-plane rollback api can restore the run to this point.
/// completes inline with no parking.
pub(super) struct CheckpointOp;

impl CheckpointOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
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
        let params: Value = ctx.node.parameters.clone().into();
        let name = parse_checkpoint_name(&params, &ctx.node.id);
        let snapshot = runinator_models::json!({
            "name": name,
            "workflow_run_id": ctx.workflow_run.id,
            "active_node_id": ctx.workflow_run.active_node_id,
            "execution_state": ctx.workflow_run.execution_state,
            "captured_at": Utc::now().timestamp(),
        });
        let inserted = ctx
            .db
            .create_automation_record(RECORD_TYPE.into(), snapshot)
            .await?;
        let checkpoint_id = inserted
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Uuid>().ok());
        let output = CheckpointOutput {
            name,
            checkpoint_id,
        };
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("checkpoint_saved".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
