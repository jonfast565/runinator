use super::context::is_reentry_stale;
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;

const RECORD_TYPE: &str = "workflow_barrier";
const DEFAULT_POLL_INTERVAL: i64 = 5;

/// true when enough runs have arrived to satisfy the barrier.
pub(super) fn arrivals_complete(count: usize, expected: i64) -> bool {
    expected > 0 && count as i64 >= expected
}

fn parse_barrier_params(node: &WorkflowNode) -> (String, i64, i64) {
    let params: Value = node.parameters.clone().into();
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&node.id)
        .to_string();
    let expected = params.get("count").and_then(Value::as_i64).unwrap_or(1);
    let poll_interval = params
        .get("poll_interval_seconds")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_POLL_INTERVAL);
    (name, expected, poll_interval)
}

async fn fetch_barrier_record<T: RuntimeStore>(
    ctx: &super::execution::NodeExecutionContext<'_, T>,
    name: &str,
) -> Result<Option<Value>, SendableError> {
    let records = ctx
        .db
        .fetch_automation_records(RECORD_TYPE.into(), None, None)
        .await?;
    Ok(records
        .into_iter()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name)))
}

/// register this run's arrival at the barrier and return the updated arrivals list.
async fn register_arrival<T: RuntimeStore>(
    ctx: &super::execution::NodeExecutionContext<'_, T>,
    name: &str,
    expected: i64,
) -> Result<Vec<Uuid>, SendableError> {
    match fetch_barrier_record(ctx, name).await? {
        None => {
            let record = runinator_models::json!({
                "name": name,
                "expected_count": expected,
                "arrivals": [ctx.workflow_run.id],
            });
            ctx.db
                .create_automation_record(RECORD_TYPE.into(), record)
                .await?;
            Ok(vec![ctx.workflow_run.id])
        }
        Some(record) => {
            let mut arrivals: Vec<Uuid> = record
                .get("arrivals")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_str().and_then(|s| s.parse::<Uuid>().ok()))
                .collect();
            if !arrivals.contains(&ctx.workflow_run.id) {
                arrivals.push(ctx.workflow_run.id);
                let record_id = record
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<Uuid>().ok());
                if let Some(id) = record_id {
                    let mut updated = record.clone();
                    if let Some(obj) = updated.as_object_mut() {
                        let arr: Vec<Value> = arrivals
                            .iter()
                            .map(|u| Value::String(u.to_string()))
                            .collect();
                        obj.insert("arrivals".into(), Value::Array(arr));
                    }
                    ctx.db
                        .update_automation_record(RECORD_TYPE.into(), id, updated)
                        .await?;
                }
            }
            Ok(arrivals)
        }
    }
}

async fn enqueue_barrier_poll<T: RuntimeStore>(
    ctx: &super::execution::NodeExecutionContext<'_, T>,
    interval: i64,
) -> Result<(), SendableError> {
    let poll_at = Utc::now() + chrono::Duration::seconds(interval);
    let event = NewOrchestrationEvent::new(
        ctx.workflow_run.id,
        Some(ctx.node.id.clone()),
        "barrier_poll",
        runinator_models::json!({ "node_id": ctx.node.id }),
    );
    ctx.db
        .enqueue_ready_node(event, ctx.node.id.clone(), poll_at)
        .await?;
    Ok(())
}

/// process a barrier node: register this run's arrival and park until N runs have all arrived.
/// the last arrival wakes all others via their poll loop (or the others wake naturally on their
/// next poll interval).
pub(super) struct BarrierOp;

impl BarrierOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        let (name, expected, poll_interval) = parse_barrier_params(ctx.node);
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));

        if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
            if timed_out_since_created(ctx.timing(), node_run) {
                time_out(ctx, node_run, "Barrier timed out").await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            // re-check the barrier record for new arrivals.
            let arrivals = register_arrival(ctx, &name, expected).await?;
            if arrivals_complete(arrivals.len(), expected) {
                let output = BarrierOutput {
                    name: name.clone(),
                    arrivals: arrivals.clone(),
                };
                transition_from_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("barrier_released".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            enqueue_barrier_poll(ctx, poll_interval).await?;
            return Ok(ReadyNodeDisposition::KeepClaim);
        }

        // first visit: register and check immediately.
        let arrivals = register_arrival(ctx, &name, expected).await?;
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
        if arrivals_complete(arrivals.len(), expected) {
            let output = BarrierOutput {
                name: name.clone(),
                arrivals: arrivals.clone(),
            };
            transition_from_node(
                ctx,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("barrier_released".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        let state = BarrierState {
            name: name.clone(),
            expected_count: expected,
            arrivals: arrivals.clone(),
            deadline_unix: ctx.node.timeout_seconds.map(|t| Utc::now().timestamp() + t),
        };
        ctx.db
            .update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Waiting,
                Some(node_run.attempt + 1),
                None,
                None,
                Some(state.to_wire_value()?),
                Some("barrier_waiting".into()),
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
        enqueue_barrier_poll(ctx, poll_interval).await?;
        arm_node_timeout(ctx).await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
