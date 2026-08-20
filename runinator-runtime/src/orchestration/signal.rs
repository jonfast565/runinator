use super::context::{is_reentry_stale, runtime_context};
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;

/// process a signal node: park the run until a named external signal is delivered. purely
/// event-driven (no polling) — the delivery endpoint sets the node run to `Succeeded` with the
/// signal payload and wakes the runtime, which then follows the success edge. mirrors `approval`
/// (park + arm_node_timeout + out-of-band resolution), but resolved by an arbitrary signal rather
/// than a human decision. the optional node timeout fails the wait via `on_timeout`/`on_failure`.
pub(super) struct SignalOp;

impl SignalOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        // a loop body re-entering this node sees the prior iteration's resolved run; treat it as a fresh
        // visit so a new wait is armed instead of transitioning from the stale run.
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));
        if let Some(node_run) = latest {
            if node_run.status == WorkflowStatus::Waiting
                && timed_out_since_created(ctx.timing(), node_run)
            {
                return super::execution::complete(
                    time_out(ctx, node_run, "Signal timed out").await,
                );
            }
            // the delivery endpoint stamps the node run `Succeeded` with the payload; follow the edge.
            if node_run.status == WorkflowStatus::Succeeded {
                transition_from_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    node_run.output_json.clone(),
                    Some("signal_received".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            return Ok(ReadyNodeDisposition::Complete);
        }

        // first visit: park on the named signal and arm the optional timeout.
        let params = runinator_workflows::parse_signal_parameters(ctx.node);
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
        let correlation_key = resolve_correlation_key(ctx, &params.correlation_key).await;
        let state = SignalState {
            name: params.name,
            correlation_key,
        };
        ctx.db
            .update_workflow_node_run(
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
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::Waiting,
                Some(ctx.node.id.clone()),
                None,
                None,
            )
            .await?;
        super::execution::complete(arm_node_timeout(ctx).await)
    }
}

/// resolve a signal node's correlation-key value (often a `$ref` into the run context) into a flat
/// string. a null/empty key yields `None`; numbers and other scalars coerce to their string form so
/// an external webhook can match a ticket key, PR number, etc.
async fn resolve_correlation_key<T: RuntimeStore>(
    ctx: &super::execution::NodeExecutionContext<'_, T>,
    expression: &runinator_models::workflow_ast::WorkflowExpression,
) -> Option<String> {
    use runinator_models::workflow_ast::WorkflowExpression;
    if matches!(expression, WorkflowExpression::Literal(Value::Null)) {
        return None;
    }
    let context = runtime_context(ctx).await;
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
