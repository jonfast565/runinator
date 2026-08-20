use super::context::runtime_context;
use super::transitions::transition_from_node;
use super::*;

/// process an assert node: evaluates all named assertions inline; fails with a structured
/// violation list if any assertion does not hold.
pub(super) struct AssertOp;

impl AssertOp {
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
        let context = runtime_context(ctx).await;
        let params: Value = ctx.node.parameters.clone().into();
        let violations = runinator_workflows::evaluate_assertions(&params, &context);
        let passed = violations.is_empty();
        let output = AssertOutput { passed, violations };
        let (status, reason) = if passed {
            (WorkflowStatus::Succeeded, "assert_passed")
        } else {
            (WorkflowStatus::Failed, "assert_failed")
        };
        transition_from_node(
            ctx,
            &node_run,
            status,
            Some(output.to_wire_value()?),
            Some(reason.into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
