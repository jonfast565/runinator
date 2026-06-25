use super::context::is_reentry_stale;
use super::transitions::{arm_node_timeout, time_out, timed_out, transition_from_node};
use super::*;

const RECORD_TYPE: &str = "workflow_throttle";
const DEFAULT_POLL_INTERVAL: i64 = 5;

struct ThrottleParams {
    name: String,
    max_per_window: i64,
    window_seconds: i64,
    poll_interval: i64,
}

fn parse_throttle_params(node: &WorkflowNode) -> ThrottleParams {
    let params: Value = node.parameters.clone().into();
    ThrottleParams {
        name: params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&node.id)
            .to_string(),
        max_per_window: params
            .get("max_per_window")
            .and_then(Value::as_i64)
            .unwrap_or(10),
        window_seconds: params
            .get("window_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(60),
        poll_interval: params
            .get("poll_interval_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_POLL_INTERVAL),
    }
}

/// true when the throttle bucket has at least one token remaining in the current window.
pub(super) fn bucket_has_tokens(record: &Value, max_per_window: i64, window_seconds: i64) -> bool {
    let window_start = record
        .get("window_start")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let tokens_used = record
        .get("tokens_used")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let now = Utc::now().timestamp();
    if now - window_start >= window_seconds {
        // window expired; a fresh window always has tokens.
        return true;
    }
    tokens_used < max_per_window
}

async fn fetch_bucket<T: DatabaseImpl>(db: &T, name: &str) -> Result<Option<Value>, SendableError> {
    let records = db
        .fetch_automation_records(RECORD_TYPE.into(), None, None)
        .await?;
    Ok(records
        .into_iter()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name)))
}

async fn consume_token<T: DatabaseImpl>(
    db: &T,
    name: &str,
    max_per_window: i64,
    window_seconds: i64,
) -> Result<bool, SendableError> {
    let existing = fetch_bucket(db, name).await?;
    let now = Utc::now().timestamp();
    match existing {
        None => {
            let record = runinator_models::json!({
                "name": name,
                "tokens_used": 1,
                "max_per_window": max_per_window,
                "window_seconds": window_seconds,
                "window_start": now,
            });
            db.create_automation_record(RECORD_TYPE.into(), record)
                .await?;
            Ok(true)
        }
        Some(record) => {
            if !bucket_has_tokens(&record, max_per_window, window_seconds) {
                return Ok(false);
            }
            let record_id = record
                .get("id")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<Uuid>().ok());
            if let Some(id) = record_id {
                let window_start = record
                    .get("window_start")
                    .and_then(Value::as_i64)
                    .unwrap_or(now);
                let tokens_used = if now - window_start >= window_seconds {
                    0i64
                } else {
                    record
                        .get("tokens_used")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                };
                let mut updated = record.clone();
                if let Some(obj) = updated.as_object_mut() {
                    obj.insert("tokens_used".into(), (tokens_used + 1).into());
                    if now - window_start >= window_seconds {
                        obj.insert("window_start".into(), now.into());
                    }
                }
                db.update_automation_record(RECORD_TYPE.into(), id, updated)
                    .await?;
            }
            Ok(true)
        }
    }
}

async fn enqueue_throttle_poll<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
    interval: i64,
) -> Result<(), SendableError> {
    let poll_at = Utc::now() + chrono::Duration::seconds(interval);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "throttle_poll",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), poll_at)
        .await?;
    Ok(())
}

/// process a throttle node: consume one token from a named sliding-window bucket. parks and polls
/// until a token is available or the optional timeout elapses.
pub(super) async fn process_throttle_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<ReadyNodeDisposition, SendableError> {
    let params = parse_throttle_params(node);
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));

    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        if timed_out(node, node_run) {
            time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Throttle timed out",
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        if consume_token(
            db,
            &params.name,
            params.max_per_window,
            params.window_seconds,
        )
        .await?
        {
            let output = ThrottleOutput {
                name: params.name,
                admitted: true,
            };
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("throttle_admitted".into()),
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        enqueue_throttle_poll(db, workflow_run.id, node, params.poll_interval).await?;
        return Ok(ReadyNodeDisposition::KeepClaim);
    }

    // first visit.
    if consume_token(
        db,
        &params.name,
        params.max_per_window,
        params.window_seconds,
    )
    .await?
    {
        let node_run = db
            .create_workflow_node_run(
                workflow_run.id,
                node.id.clone(),
                node.parameters.clone().into(),
            )
            .await?;
        let output = ThrottleOutput {
            name: params.name,
            admitted: true,
        };
        transition_from_node(
            db,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("throttle_admitted".into()),
            node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    // bucket exhausted; park.
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let state = ThrottleState {
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
        Some("throttle_waiting".into()),
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
    enqueue_throttle_poll(db, workflow_run.id, node, params.poll_interval).await?;
    arm_node_timeout(db, workflow_run.id, node).await?;
    Ok(ReadyNodeDisposition::Complete)
}

pub(super) struct ThrottleHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for ThrottleHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_throttle_node(
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
