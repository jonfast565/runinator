use super::context::is_reentry_stale;
use super::transitions::{time_out, timed_out_since_created, transition_from_node};
use super::*;

/// true when the debounce deadline has elapsed (i.e. no new trigger has reset it).
pub(super) fn deadline_elapsed(deadline_unix: i64) -> bool {
    Utc::now().timestamp() >= deadline_unix
}

fn parse_delay_seconds(node: &WorkflowNode) -> i64 {
    let params: Value = node.parameters.clone().into();
    params
        .get("delay_seconds")
        .and_then(Value::as_i64)
        .unwrap_or(30)
}

async fn enqueue_debounce_poll<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
    deadline_unix: i64,
) -> Result<(), SendableError> {
    let poll_at =
        chrono::DateTime::<Utc>::from_timestamp(deadline_unix, 0).unwrap_or_else(Utc::now);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "debounce_poll",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), poll_at)
        .await?;
    Ok(())
}

/// process a debounce node: parks for a trailing delay window. if the run is re-driven before the
/// deadline (via a reset call on the api) the deadline is pushed forward. once the deadline
/// elapses without a reset the node succeeds.
///
/// external reset: `POST /workflow_runs/{id}/debounce/{node_id}/reset` should update the
/// `DebounceState.deadline_unix` in the node run state and re-enqueue at the new deadline.
pub(super) async fn process_debounce_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<ReadyNodeDisposition, SendableError> {
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));

    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        if timed_out_since_created(node, node_run) {
            time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Debounce timed out",
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        let state = node_run.state.decode::<DebounceState>().ok();
        let deadline = state.as_ref().map(|s| s.deadline_unix).unwrap_or(i64::MAX);
        if !deadline_elapsed(deadline) {
            // re-arm at the current deadline (it may have been pushed by an external reset).
            enqueue_debounce_poll(db, workflow_run.id, node, deadline).await?;
            return Ok(ReadyNodeDisposition::KeepClaim);
        }
        let output = DebounceOutput {
            deadline_unix: deadline,
        };
        let all_node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
        transition_from_node(
            db,
            workflow_run,
            node,
            node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("debounce_elapsed".into()),
            &all_node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    // first visit: park at the trailing deadline.
    let delay = parse_delay_seconds(node);
    let deadline = Utc::now().timestamp() + delay;
    let params: Value = node.parameters.clone().into();
    let state = DebounceState {
        deadline_unix: deadline,
        trigger_key: params
            .get("trigger_key")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.to_wire_value()?),
        Some("debounce_started".into()),
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
    enqueue_debounce_poll(db, workflow_run.id, node, deadline).await?;
    Ok(ReadyNodeDisposition::Complete)
}

pub(super) struct DebounceHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for DebounceHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_debounce_node(
                ctx.db,
                ctx.workflow_run,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await
        }
    }
}
