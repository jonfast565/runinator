use super::context::is_reentry_stale;
use super::transitions::{arm_node_timeout, time_out, timed_out, transition_from_node};
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

async fn enqueue_collect_deadline<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
    deadline_unix: i64,
) -> Result<(), SendableError> {
    let ready_at =
        chrono::DateTime::<Utc>::from_timestamp(deadline_unix, 0).unwrap_or_else(Utc::now);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "collect_timeout",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), ready_at)
        .await?;
    Ok(())
}

/// process a collect node: parks and accumulates items delivered via an external api endpoint.
/// succeeds when either the item count reaches `max` or the timeout elapses. delivery endpoint:
/// `POST /workflow_runs/{id}/collect/{node_id}/items`.
pub(super) async fn process_collect_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<ReadyNodeDisposition, SendableError> {
    let (name, threshold, _timeout) = parse_collect_params(node);
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));

    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        if timed_out(node, node_run) {
            // emit whatever was collected before timing out.
            let state = serde_json::from_value::<CollectState>(node_run.state.clone().into()).ok();
            let items = state.map(|s| s.items).unwrap_or_default();
            let count = items.len();
            let output = CollectOutput {
                items,
                count,
                reason: "timeout".into(),
            };
            let all_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("collect_timeout".into()),
                &all_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        // re-read state (external delivery may have appended items).
        let state = serde_json::from_value::<CollectState>(node_run.state.clone().into()).ok();
        let items = state.as_ref().map(|s| s.items.clone()).unwrap_or_default();
        if threshold_reached(items.len(), threshold) {
            let count = items.len();
            let output = CollectOutput {
                items,
                count,
                reason: "threshold".into(),
            };
            let all_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("collect_threshold_met".into()),
                &all_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        // keep waiting.
        return Ok(ReadyNodeDisposition::KeepClaim);
    }

    // first visit.
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let deadline_unix = node.timeout_seconds.map(|t| Utc::now().timestamp() + t);
    let state = CollectState {
        name: name.clone(),
        items: Vec::new(),
        threshold,
        deadline_unix,
    };
    db.update_workflow_node_run(
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
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        None,
        None,
    )
    .await?;
    if let Some(deadline) = deadline_unix {
        enqueue_collect_deadline(db, workflow_run.id, node, deadline).await?;
    }
    arm_node_timeout(db, workflow_run.id, node).await?;
    Ok(ReadyNodeDisposition::Complete)
}
