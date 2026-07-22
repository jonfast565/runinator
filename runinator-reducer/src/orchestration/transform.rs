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
pub(super) async fn process_transform_node<T: DatabaseImpl>(
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
            super::context::most_recently_finished_node_run(node_runs),
        )
        .await?;
    let context = runtime_context(db, workflow_run, node_runs).await;
    let params: Value = node.parameters.clone().into();
    let bindings = resolve_bindings(&params, &context);
    let output = TransformOutput { bindings };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some("transform_applied".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) struct TransformHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for TransformHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_transform_node(ctx.db, ctx.workflow_run, ctx.node, ctx.node_runs).await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}
