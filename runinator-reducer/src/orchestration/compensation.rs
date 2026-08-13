use super::context::runtime_context;
use super::*;
use runinator_models::workflows::WorkflowDefinition;
use uuid::Uuid;

// synthetic node-id prefix for a compensation action run; never a real graph node, only a carrier so
// the action executes through the normal worker path while the run stays parked on the fail node.
const COMPENSATION_NODE_PREFIX: &str = "__compensate__";

/// process the `fail` terminal with saga rollback. on first arrival it gathers the compensations of
/// every succeeded node (most-recently-completed first) and unwinds them one at a time through the
/// action outbox; once the stack is empty the run finalizes `Failed`. compensation is best-effort:
/// a failed compensation action does not stop the unwind.
pub(super) struct FailHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for FailHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let mut run_state = ctx.run_state_snapshot().clone();

        // first arrival: decide whether there is anything to unwind.
        let mut frame = match run_state.compensation.take() {
            Some(frame) => frame,
            None => {
                let remaining = collect_compensations(ctx.workflow, ctx.node_runs);
                if remaining.is_empty() {
                    return finalize_fail(ctx).await;
                }
                CompensationFrame {
                    remaining,
                    active_run_id: None,
                }
            }
        };

        // a compensation action is in flight: wait for it, then drop it from the stack (best-effort).
        if let Some(active_run_id) = frame.active_run_id {
            match ctx.node_runs.iter().find(|run| run.id == active_run_id) {
                Some(run) if run.status.is_terminal() => frame.active_run_id = None,
                Some(_) => {
                    // still running; re-persist and keep the claim until the worker result wakes us.
                    run_state.compensation = Some(frame);
                    persist_frame(ctx, &run_state).await?;
                    return Ok(ReadyNodeDisposition::KeepClaim);
                }
                // the run vanished (should not happen); treat as finished so we make progress.
                None => frame.active_run_id = None,
            }
        }

        // dispatch the next compensation if any remain.
        if !frame.remaining.is_empty() {
            let origin = frame.remaining.remove(0);
            dispatch_compensation(ctx, &origin, &mut frame).await?;
            run_state.compensation = Some(frame);
            persist_frame(ctx, &run_state).await?;
            return Ok(ReadyNodeDisposition::KeepClaim);
        }

        // stack drained: clear the frame and finalize the failure.
        run_state.compensation = None;
        persist_frame(ctx, &run_state).await?;
        finalize_fail(ctx).await
    }
}

/// gather the origin node ids of succeeded nodes that declare a compensation, ordered most-recently
/// completed first (so the unwind runs in reverse of execution). each origin appears once.
fn collect_compensations(
    workflow: &WorkflowDefinition,
    node_runs: &[WorkflowNodeRun],
) -> Vec<String> {
    let has_compensation = |node_id: &str| {
        workflow
            .definition
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .is_some_and(|node| node.compensation.is_some())
    };
    let mut succeeded: Vec<&WorkflowNodeRun> = node_runs
        .iter()
        .filter(|run| run.status == WorkflowStatus::Succeeded && has_compensation(&run.node_id))
        .collect();
    // most recent completion first; fall back to created order when finish times tie or are absent.
    succeeded.sort_by(|a, b| {
        b.finished_at
            .cmp(&a.finished_at)
            .then(b.created_at.cmp(&a.created_at))
    });
    let mut ordered = Vec::new();
    for run in succeeded {
        if !ordered.contains(&run.node_id) {
            ordered.push(run.node_id.clone());
        }
    }
    ordered
}

/// dispatch one compensation action through the action outbox, recording the synthetic run on the
/// frame. parameters are resolved against the live context so a rollback can read the origin node's
/// output (e.g. a created resource id).
async fn dispatch_compensation<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    origin: &str,
    frame: &mut CompensationFrame,
) -> Result<(), SendableError> {
    let Some(action) = ctx
        .workflow
        .definition
        .nodes
        .iter()
        .find(|node| node.id == origin)
        .and_then(|node| node.compensation.clone())
    else {
        // no compensation after all; skip without parking.
        return Ok(());
    };
    let context = runtime_context(ctx).await;
    let parameters =
        runinator_workflows::resolve_value_refs(action.configuration.as_value(), &context)
            .unwrap_or_else(|_| action.configuration.as_value().clone());

    let synthetic_id = format!("{COMPENSATION_NODE_PREFIX}:{origin}");
    let node_run = ctx
        .db
        .create_workflow_node_run(
            ctx.workflow_run.id,
            synthetic_id.clone(),
            parameters.clone(),
            super::context::most_recently_finished_node_run(ctx.node_runs),
            Some(ctx.cursor),
        )
        .await?;
    let command = ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: ctx.workflow_run.id,
        workflow_node_run_id: node_run.id,
        node_id: synthetic_id,
        action,
        attempt: 1,
        parameters,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: runinator_utilities::telemetry::current_trace_context(),
        notification_delivery_id: None,
        // a compensation is the undo of an effect that already happened, so it is its own effect and
        // must not share the forward action's key. declaring one on the compensating handler itself
        // is the way to make an undo idempotent.
        idempotency_key: None,
    };
    ctx.db
        .enqueue_action_dispatch(
            format!("compensation:{}:{}", ctx.workflow_run.id, node_run.id),
            command,
        )
        .await?;
    ctx.db
        .update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Running,
            Some(node_run.attempt + 1),
            None,
            None,
            None,
            Some("compensation_started".into()),
            Some(format!("Compensating {origin}")),
        )
        .await?;
    frame.active_run_id = Some(node_run.id);
    Ok(())
}

/// persist the run state while keeping the run parked on the fail node, `Running`.
async fn persist_frame<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    run_state: &WorkflowRunState,
) -> Result<(), SendableError> {
    ctx.db
        .update_workflow_run_status(
            ctx.workflow_run.id,
            WorkflowStatus::Running,
            Some(ctx.node.id.clone()),
            Some(run_state.clone()),
            Some("Compensating before failure".into()),
        )
        .await
}

/// the original `fail` terminal behavior: mark the fail node-run complete and the run `Failed`.
async fn finalize_fail<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
) -> Result<ReadyNodeDisposition, SendableError> {
    super::transitions::ensure_completed_node_run(ctx, "fail_reached").await?;
    // a failing terminal ends the whole run: `advance_cursor` drains every thread of control, so no
    // sibling branch keeps walking after the run has failed.
    run_state::advance_cursor(
        ctx.db,
        ctx.workflow_run.id,
        ctx.cursor.id,
        WorkflowStatus::Failed,
        run_state::CursorMove::Retire,
        Some("Workflow reached fail node".into()),
    )
    .await?;
    Ok(ReadyNodeDisposition::Complete)
}
