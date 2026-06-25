use super::context::is_reentry_stale;
use super::transitions::{arm_node_timeout, time_out, timed_out, transition_from_node};
use super::*;

const DEFAULT_POLL_INTERVAL: i64 = 10;

struct AwaitParams {
    run_ids: Vec<Uuid>,
    mode: String,
    poll_interval: i64,
}

/// parse run_ids and mode from the node's parameters. run_ids may be uuids or uuid strings.
pub(super) fn parse_await_mode(params: &Value) -> String {
    params
        .get("mode")
        .and_then(Value::as_str)
        .filter(|m| matches!(*m, "all" | "any"))
        .unwrap_or("all")
        .to_string()
}

fn parse_await_params(node: &WorkflowNode) -> AwaitParams {
    let params: Value = node.parameters.clone().into();
    let run_ids = params
        .get("run_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().and_then(|s| s.parse::<Uuid>().ok()))
        .collect();
    AwaitParams {
        run_ids,
        mode: parse_await_mode(&params),
        poll_interval: params
            .get("poll_interval_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_POLL_INTERVAL),
    }
}

async fn check_run_statuses<T: DatabaseImpl>(
    db: &T,
    run_ids: &[Uuid],
) -> Result<Vec<(Uuid, WorkflowStatus)>, SendableError> {
    let mut results = Vec::with_capacity(run_ids.len());
    for &id in run_ids {
        if let Some(run) = db.fetch_workflow_run(id).await? {
            results.push((id, run.status));
        }
    }
    Ok(results)
}

fn satisfaction_met(statuses: &[(Uuid, WorkflowStatus)], mode: &str) -> bool {
    match mode {
        "any" => statuses.iter().any(|(_, s)| s.is_terminal()),
        _ => statuses.iter().all(|(_, s)| s.is_terminal()),
    }
}

async fn enqueue_await_poll<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
    interval: i64,
) -> Result<(), SendableError> {
    let poll_at = Utc::now() + chrono::Duration::seconds(interval);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "await_run_poll",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), poll_at)
        .await?;
    Ok(())
}

/// process an await_run node: poll sibling run(s) until a satisfaction policy is met. parks
/// between polls; does not change the target runs.
pub(super) async fn process_await_run_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<ReadyNodeDisposition, SendableError> {
    let params = parse_await_params(node);
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));

    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        if timed_out(node, node_run) {
            time_out(
                db,
                workflow_run,
                node,
                node_run,
                "AwaitRun timed out",
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        let statuses = check_run_statuses(db, &params.run_ids).await?;
        if satisfaction_met(&statuses, &params.mode) {
            let output = AwaitRunOutput {
                run_ids: params.run_ids.clone(),
                mode: params.mode.clone(),
                statuses: statuses
                    .iter()
                    .map(|(_, s)| s.as_str().to_string())
                    .collect(),
            };
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("await_run_satisfied".into()),
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        enqueue_await_poll(db, workflow_run.id, node, params.poll_interval).await?;
        return Ok(ReadyNodeDisposition::KeepClaim);
    }

    // first visit: check immediately before parking.
    let statuses = check_run_statuses(db, &params.run_ids).await?;
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    if satisfaction_met(&statuses, &params.mode) {
        let output = AwaitRunOutput {
            run_ids: params.run_ids.clone(),
            mode: params.mode.clone(),
            statuses: statuses
                .iter()
                .map(|(_, s)| s.as_str().to_string())
                .collect(),
        };
        transition_from_node(
            db,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("await_run_satisfied".into()),
            node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }
    let state = AwaitRunState {
        run_ids: params.run_ids.clone(),
        mode: params.mode.clone(),
        poll_interval: params.poll_interval,
        deadline_unix: node.timeout_seconds.map(|t| Utc::now().timestamp() + t),
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.to_wire_value()?),
        Some("await_run_waiting".into()),
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
    enqueue_await_poll(db, workflow_run.id, node, params.poll_interval).await?;
    arm_node_timeout(db, workflow_run.id, node).await?;
    Ok(ReadyNodeDisposition::Complete)
}

pub(super) struct AwaitRunHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for AwaitRunHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_await_run_node(
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
