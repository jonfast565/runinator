use super::transitions::transition_from_node;
use super::*;

const RECORD_TYPE: &str = "workflow_cooldown";

struct CooldownParams {
    name: String,
    window_seconds: i64,
}

fn parse_cooldown_params(node: &WorkflowNode) -> CooldownParams {
    let params: Value = node.parameters.clone().into();
    CooldownParams {
        name: params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&node.id)
            .to_string(),
        window_seconds: params
            .get("window_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(60),
    }
}

async fn fetch_record<T: DatabaseImpl>(db: &T, name: &str) -> Result<Option<Value>, SendableError> {
    let records = db
        .fetch_automation_records(RECORD_TYPE.into(), None, None)
        .await?;
    Ok(records
        .into_iter()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name)))
}

/// seconds left in the cooldown window for a named record; 0 once the window has elapsed.
pub(super) fn remaining_seconds(record: &Value, window_seconds: i64, now_unix: i64) -> i64 {
    let last_run_at = record
        .get("last_run_at")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    (last_run_at + window_seconds - now_unix).max(0)
}

/// stamp `last_run_at = now` on the named record, creating it on first use, so the next pass within
/// the window is short-circuited.
async fn stamp_cooldown<T: DatabaseImpl>(
    db: &T,
    name: &str,
    existing: Option<&Value>,
) -> Result<(), SendableError> {
    let now = Utc::now().timestamp();
    match existing {
        None => {
            let record = runinator_models::json!({ "name": name, "last_run_at": now });
            db.create_automation_record(RECORD_TYPE.into(), record)
                .await?;
        }
        Some(record) => {
            let record_id = record
                .get("id")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<Uuid>().ok());
            let mut updated = record.clone();
            if let Some(obj) = updated.as_object_mut() {
                obj.insert("last_run_at".into(), now.into());
            }
            if let Some(id) = record_id {
                db.update_automation_record(RECORD_TYPE.into(), id, updated)
                    .await?;
            }
        }
    }
    Ok(())
}

/// process a cooldown node: a named cross-run gate. if the prior pass ran within the window, the run
/// is completed as `Succeeded` without entering the body (a clean no-op). otherwise the window is
/// stamped and the node proceeds via `on_success` into the body.
pub(super) async fn process_cooldown_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = parse_cooldown_params(node);
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
            super::context::most_recently_finished_node_run(node_runs),
        )
        .await?;
    let now = Utc::now().timestamp();
    let record = fetch_record(db, &params.name).await?;
    let remaining = record
        .as_ref()
        .map(|r| remaining_seconds(r, params.window_seconds, now))
        .unwrap_or(0);

    // still inside the window: short-circuit the run to success without touching the body.
    if remaining > 0 {
        let output = CooldownOutput {
            name: params.name.clone(),
            skipped: true,
            remaining_seconds: remaining,
        };
        db.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Succeeded,
            Some(node_run.attempt + 1),
            None,
            Some(output.to_wire_value()?),
            None,
            Some("cooldown_skipped".into()),
            None,
        )
        .await?;
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Succeeded,
            Some(node.id.clone()),
            None,
            None,
        )
        .await?;
        return Ok(());
    }

    // window elapsed: record this pass and proceed into the body.
    stamp_cooldown(db, &params.name, record.as_ref()).await?;
    let output = CooldownOutput {
        name: params.name.clone(),
        skipped: false,
        remaining_seconds: 0,
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some("cooldown_passed".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) struct CooldownHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for CooldownHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_cooldown_node(ctx.db, ctx.workflow_run, ctx.node, ctx.node_runs).await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}
