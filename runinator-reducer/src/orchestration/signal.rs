use super::context::{is_reentry_stale, runtime_context};
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;

/// process a signal node: park the run until a named external signal is delivered. purely
/// event-driven (no polling) — the delivery endpoint sets the node run to `Succeeded` with the
/// signal payload and wakes the reducer, which then follows the success edge. mirrors `approval`
/// (park + arm_node_timeout + out-of-band resolution), but resolved by an arbitrary signal rather
/// than a human decision. the optional node timeout fails the wait via `on_timeout`/`on_failure`.
pub(super) async fn process_signal_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    // a loop body re-entering this node sees the prior iteration's resolved run; treat it as a fresh
    // visit so a new wait is armed instead of transitioning from the stale run.
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::Waiting && timed_out_since_created(node, node_run) {
            return time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Signal timed out",
                node_runs,
            )
            .await;
        }
        // the delivery endpoint stamps the node run `Succeeded` with the payload; follow the edge.
        if node_run.status == WorkflowStatus::Succeeded {
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                node_run.output_json.clone(),
                Some("signal_received".into()),
                node_runs,
            )
            .await?;
            return Ok(());
        }
        return Ok(());
    }

    // first visit: park on the named signal and arm the optional timeout.
    let params = runinator_workflows::parse_signal_parameters(node);
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
            super::context::most_recently_finished_node_run(node_runs),
        )
        .await?;
    let correlation_key =
        resolve_correlation_key(db, workflow_run, node_runs, &params.correlation_key).await;
    let state = SignalState {
        name: params.name,
        correlation_key,
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.to_wire_value()?),
        Some("signal_waiting".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        None,
        None,
    )
    .await?;
    arm_node_timeout(db, workflow_run.id, node).await
}

/// resolve a signal node's correlation-key value (often a `$ref` into the run context) into a flat
/// string. a null/empty key yields `None`; numbers and other scalars coerce to their string form so
/// an external webhook can match a ticket key, PR number, etc.
async fn resolve_correlation_key<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
    expression: &runinator_models::workflow_ast::WorkflowExpression,
) -> Option<String> {
    use runinator_models::workflow_ast::WorkflowExpression;
    if matches!(expression, WorkflowExpression::Literal(Value::Null)) {
        return None;
    }
    let context = runtime_context(db, workflow_run, node_runs).await;
    let resolved = runinator_workflows::evaluate_expression(expression, &context)
        .unwrap_or_else(|_| Value::from(expression));
    if let Some(text) = resolved.as_str() {
        return Some(text.to_string());
    }
    if let Some(int) = resolved.as_i64() {
        return Some(int.to_string());
    }
    if resolved.is_null() {
        return None;
    }
    Some(resolved.to_string())
}

pub(super) struct SignalHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for SignalHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_signal_node(
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
