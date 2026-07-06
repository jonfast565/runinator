use super::context::is_reentry_stale;
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;

const RECORD_TYPE: &str = "workflow_mutex";
const DEFAULT_POLL_INTERVAL: i64 = 5;

struct MutexParams {
    name: String,
    poll_interval: i64,
}

fn parse_mutex_params(node: &WorkflowNode) -> MutexParams {
    let params: Value = node.parameters.clone().into();
    MutexParams {
        name: params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&node.id)
            .to_string(),
        poll_interval: params
            .get("poll_interval_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_POLL_INTERVAL),
    }
}

/// true when a mutex automation record is currently held by a run other than `skip_run_id`.
/// a record is considered released when it carries a `released_at` field.
pub(super) fn record_is_held_by_other(record: &Value, skip_run_id: Uuid) -> bool {
    if record.get("released_at").is_some() {
        return false;
    }
    record
        .get("held_by_run_id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Uuid>().ok())
        .is_some_and(|id| id != skip_run_id)
}

async fn mutex_is_locked<T: DatabaseImpl>(
    db: &T,
    name: &str,
    skip_run_id: Uuid,
) -> Result<bool, SendableError> {
    let records = db
        .fetch_automation_records(RECORD_TYPE.into(), None, None)
        .await?;
    Ok(records.iter().any(|r| {
        r.get("name").and_then(Value::as_str) == Some(name)
            && record_is_held_by_other(r, skip_run_id)
    }))
}

async fn acquire_mutex<T: DatabaseImpl>(
    db: &T,
    name: &str,
    run_id: Uuid,
) -> Result<Option<Uuid>, SendableError> {
    let record = runinator_models::json!({
        "name": name,
        "held_by_run_id": run_id,
        "acquired_at": Utc::now().timestamp(),
    });
    let inserted = db
        .create_automation_record(RECORD_TYPE.into(), record)
        .await?;
    Ok(inserted
        .get("id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Uuid>().ok()))
}

async fn enqueue_mutex_poll<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
    interval: i64,
) -> Result<(), SendableError> {
    let poll_at = Utc::now() + chrono::Duration::seconds(interval);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "mutex_poll",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), poll_at)
        .await?;
    Ok(())
}

/// process a mutex node: try to acquire a named distributed lease. parks and polls until the
/// lease becomes available or the optional timeout elapses.
pub(super) async fn process_mutex_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<ReadyNodeDisposition, SendableError> {
    let params = parse_mutex_params(node);
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));

    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        if timed_out_since_created(node, node_run) {
            time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Mutex timed out",
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        if mutex_is_locked(db, &params.name, workflow_run.id).await? {
            enqueue_mutex_poll(db, workflow_run.id, node, params.poll_interval).await?;
            return Ok(ReadyNodeDisposition::KeepClaim);
        }
        // lock is free; record the acquisition and succeed.
        acquire_mutex(db, &params.name, workflow_run.id).await?;
        let output = MutexOutput {
            name: params.name,
            acquired: true,
        };
        transition_from_node(
            db,
            workflow_run,
            node,
            node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("mutex_acquired".into()),
            node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    // first visit.
    if !mutex_is_locked(db, &params.name, workflow_run.id).await? {
        let node_run = db
            .create_workflow_node_run(
                workflow_run.id,
                node.id.clone(),
                node.parameters.clone().into(),
            )
            .await?;
        acquire_mutex(db, &params.name, workflow_run.id).await?;
        let output = MutexOutput {
            name: params.name,
            acquired: true,
        };
        transition_from_node(
            db,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("mutex_acquired".into()),
            node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    // park and poll.
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let state = MutexState {
        name: params.name.clone(),
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
        Some("mutex_waiting".into()),
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
    enqueue_mutex_poll(db, workflow_run.id, node, params.poll_interval).await?;
    arm_node_timeout(db, workflow_run.id, node).await?;
    Ok(ReadyNodeDisposition::Complete)
}

pub(super) struct MutexHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for MutexHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_mutex_node(
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
