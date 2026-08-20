use super::context::runtime_context;
use super::transitions::transition_from_node;
use super::*;

/// resolve the `bindings` map in a transform node's parameters against context. each value in the
/// map is a workflow expression; the resolved map becomes the node output and is addressable by
/// downstream nodes as `steps.<id>.output.bindings.<key>`.
pub(super) fn resolve_bindings(params: &Value, context: &Value) -> Value {
    let bindings = params.get("bindings").cloned().unwrap_or(Value::Null);
    runinator_workflows::resolve_value_refs(&bindings, context).unwrap_or(bindings)
}

/// process a transform node: resolve all named expression bindings against the runtime context
/// and emit the result as the node output. pure inline, no parking, no side effects.
pub(super) struct TransformOp;

impl TransformOp {
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
        let bindings = resolve_bindings(&params, &context);
        let output = TransformOutput { bindings };
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("transform_applied".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
