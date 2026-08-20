use super::context::is_reentry_stale;
use super::transitions::{arm_node_timeout, timed_out_since_created, transition_from_node};
use super::*;

/// true when the collect buffer has reached or exceeded the item threshold.
pub(super) fn threshold_reached(count: usize, threshold: i64) -> bool {
    threshold > 0 && count as i64 >= threshold
}

fn parse_collect_params(node: &WorkflowNode) -> (String, i64, Option<i64>) {
    let params: Value = node.parameters.clone().into();
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&node.id)
        .to_string();
    let threshold = params.get("max").and_then(Value::as_i64).unwrap_or(0);
    let timeout_seconds = node.timeout_seconds;
    (name, threshold, timeout_seconds)
}

async fn enqueue_collect_deadline<T: RuntimeStore>(
    ctx: &super::execution::NodeExecutionContext<'_, T>,
    deadline_unix: i64,
) -> Result<(), SendableError> {
    let ready_at =
        chrono::DateTime::<Utc>::from_timestamp(deadline_unix, 0).unwrap_or_else(Utc::now);
    let event = NewOrchestrationEvent::new(
        ctx.workflow_run.id,
        Some(ctx.node.id.clone()),
        "collect_timeout",
        runinator_models::json!({ "node_id": ctx.node.id }),
    );
    ctx.db
        .enqueue_ready_node(event, ctx.node.id.clone(), ready_at)
        .await?;
    Ok(())
}

/// process a collect node: parks and accumulates items delivered via an external api endpoint.
/// succeeds when either the item count reaches `max` or the timeout elapses. delivery endpoint:
/// `POST /workflow_runs/{id}/collect/{node_id}/items`.
pub(super) struct CollectOp;

impl CollectOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        let (name, threshold, _timeout) = parse_collect_params(ctx.node);
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));

        if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
            if timed_out_since_created(ctx.timing(), node_run) {
                // emit whatever was collected before timing out.
                let state = node_run.state.decode::<CollectState>().ok();
                let items = state.map(|s| s.items).unwrap_or_default();
                let count = items.len();
                let output = CollectOutput {
                    items,
                    count,
                    reason: "timeout".into(),
                };
                let all_runs = ctx.db.fetch_workflow_node_runs(ctx.workflow_run.id).await?;
                let transition_ctx = ctx.with_node_runs(&all_runs);
                transition_from_node(
                    &transition_ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("collect_timeout".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            // re-read state (external delivery may have appended items).
            let state = node_run.state.decode::<CollectState>().ok();
            let items = state.as_ref().map(|s| s.items.clone()).unwrap_or_default();
            if threshold_reached(items.len(), threshold) {
                let count = items.len();
                let output = CollectOutput {
                    items,
                    count,
                    reason: "threshold".into(),
                };
                let all_runs = ctx.db.fetch_workflow_node_runs(ctx.workflow_run.id).await?;
                let transition_ctx = ctx.with_node_runs(&all_runs);
                transition_from_node(
                    &transition_ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("collect_threshold_met".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            // keep waiting.
            return Ok(ReadyNodeDisposition::KeepClaim);
        }

        // first visit.
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
        let deadline_unix = ctx.node.timeout_seconds.map(|t| Utc::now().timestamp() + t);
        let state = CollectState {
            name: name.clone(),
            items: Vec::new(),
            threshold,
            deadline_unix,
        };
        ctx.db
            .update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Waiting,
                Some(node_run.attempt + 1),
                None,
                None,
                Some(state.to_wire_value()?),
                Some("collect_waiting".into()),
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
        if let Some(deadline) = deadline_unix {
            enqueue_collect_deadline(ctx, deadline).await?;
        }
        arm_node_timeout(ctx).await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
