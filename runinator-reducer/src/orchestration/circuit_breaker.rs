use super::transitions::transition_from_node;
use super::*;

const RECORD_TYPE: &str = "workflow_circuit_breaker";

struct CbParams {
    name: String,
    threshold: i64,
    window_seconds: i64,
    cooldown_seconds: i64,
}

fn parse_cb_params(node: &WorkflowNode) -> CbParams {
    let params: Value = node.parameters.clone().into();
    CbParams {
        name: params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&node.id)
            .to_string(),
        threshold: params.get("threshold").and_then(Value::as_i64).unwrap_or(5),
        window_seconds: params
            .get("window_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(60),
        cooldown_seconds: params
            .get("cooldown_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(120),
    }
}

/// true when the circuit is tripped (`"open"` state and still within its cooldown period).
pub(super) fn is_circuit_open(record: &Value, cooldown_seconds: i64, now_unix: i64) -> bool {
    let state = record
        .get("circuit_state")
        .and_then(Value::as_str)
        .unwrap_or("closed");
    if state != "open" {
        return false;
    }
    let last_tripped = record
        .get("last_tripped_at")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    now_unix - last_tripped < cooldown_seconds
}

fn failure_count_in_window(record: &Value, window_seconds: i64, now_unix: i64) -> i64 {
    let window_start = record
        .get("window_start")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if now_unix - window_start >= window_seconds {
        return 0;
    }
    record
        .get("failure_count")
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

async fn fetch_cb_record<T: DatabaseImpl>(
    db: &T,
    name: &str,
) -> Result<Option<Value>, SendableError> {
    let records = db
        .fetch_automation_records(RECORD_TYPE.into(), None, None)
        .await?;
    Ok(records
        .into_iter()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name)))
}

async fn record_failure<T: DatabaseImpl>(
    db: &T,
    name: &str,
    threshold: i64,
    window_seconds: i64,
) -> Result<(), SendableError> {
    let now = Utc::now().timestamp();
    match fetch_cb_record(db, name).await? {
        None => {
            let record = runinator_models::json!({
                "name": name,
                "circuit_state": "closed",
                "failure_count": 1,
                "window_start": now,
                "last_tripped_at": null,
            });
            db.create_automation_record(RECORD_TYPE.into(), record)
                .await?;
        }
        Some(record) => {
            let record_id = record
                .get("id")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<Uuid>().ok());
            let failures = failure_count_in_window(&record, window_seconds, now);
            let new_count = failures + 1;
            let new_state = if new_count >= threshold {
                "open"
            } else {
                "closed"
            };
            let mut updated = record.clone();
            if let Some(obj) = updated.as_object_mut() {
                obj.insert("failure_count".into(), new_count.into());
                obj.insert("circuit_state".into(), new_state.into());
                if new_state == "open" {
                    obj.insert("last_tripped_at".into(), now.into());
                }
                if failure_count_in_window(&record, window_seconds, now) == 0 {
                    obj.insert("window_start".into(), now.into());
                }
            }
            if let Some(id) = record_id {
                db.update_automation_record(RECORD_TYPE.into(), id, updated)
                    .await?;
            }
        }
    }
    Ok(())
}

/// process a circuit_breaker node: reads the circuit state for a named resource. if the circuit
/// is open (too many recent failures across all runs), routes via `on_failure`. if closed,
/// succeeds and allows the downstream body to proceed. a downstream body's failure should call
/// the record-failure api endpoint to increment the counter.
pub(super) async fn process_circuit_breaker_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = parse_cb_params(node);
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
            super::context::most_recently_finished_node_run(node_runs),
        )
        .await?;
    let now = Utc::now().timestamp();
    let cb_record = fetch_cb_record(db, &params.name).await?;
    let (tripped, circuit_state) = match &cb_record {
        None => (false, "closed".to_string()),
        Some(r) => {
            let open = is_circuit_open(r, params.cooldown_seconds, now);
            let state = if open { "open" } else { "closed" };
            (open, state.to_string())
        }
    };

    let output = CircuitBreakerOutput {
        name: params.name.clone(),
        circuit_state: circuit_state.clone(),
        tripped,
    };
    let (status, reason) = if tripped {
        // record this as a failure in the window so metrics stay accurate.
        record_failure(db, &params.name, params.threshold, params.window_seconds).await?;
        (WorkflowStatus::Failed, "circuit_open")
    } else {
        (WorkflowStatus::Succeeded, "circuit_closed")
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        status,
        Some(output.to_wire_value()?),
        Some(reason.into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) struct CircuitBreakerHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for CircuitBreakerHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_circuit_breaker_node(ctx.db, ctx.workflow_run, ctx.node, ctx.node_runs).await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}
