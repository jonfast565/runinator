use super::support;
use super::*;
use uuid::Uuid;

pub async fn upsert_catalog_item<T: DatabaseImpl>(
    db: &T,
    item: Value,
) -> Result<Value, SendableError> {
    db.upsert_catalog_item(item).await
}

pub async fn fetch_catalog_items<T: DatabaseImpl>(
    db: &T,
    item_type: Option<String>,
) -> Result<Vec<Value>, SendableError> {
    db.fetch_catalog_items(item_type).await
}

pub async fn fetch_catalog_item<T: DatabaseImpl>(
    db: &T,
    uri: String,
) -> Result<Option<Value>, SendableError> {
    db.fetch_catalog_item(uri).await
}

pub async fn create_automation_record<T: DatabaseImpl>(
    db: &T,
    record_type: &str,
    record: Value,
) -> Result<Value, SendableError> {
    db.create_automation_record(record_type.into(), record)
        .await
}

pub async fn fetch_automation_records<T: DatabaseImpl>(
    db: &T,
    record_type: &str,
    workflow_run_id: Option<Uuid>,
    external_item_id: Option<Uuid>,
) -> Result<Vec<Value>, SendableError> {
    db.fetch_automation_records(record_type.into(), workflow_run_id, external_item_id)
        .await
}

pub async fn put_idempotency_key<T: DatabaseImpl>(
    db: &T,
    scope: String,
    key: String,
    result: Value,
) -> Result<Value, SendableError> {
    db.put_idempotency_key(scope, key, result).await
}

pub async fn fetch_idempotency_key<T: DatabaseImpl>(
    db: &T,
    scope: String,
    key: String,
) -> Result<Option<Value>, SendableError> {
    db.fetch_idempotency_key(scope, key).await
}

pub async fn resolve_approval<T: DatabaseImpl>(
    db: &T,
    approval_id: Uuid,
    approved: bool,
    resolved_by: Option<String>,
    message: Option<String>,
    output_json: Option<Value>,
) -> Result<Value, SendableError> {
    let Some(mut approval) = db
        .fetch_automation_record("approval_requests".into(), approval_id)
        .await?
    else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Approval request {approval_id} not found"),
        )));
    };
    let now = Utc::now().timestamp();
    if let Some(object) = approval.as_object_mut() {
        object.insert(
            "status".into(),
            if approved { "approved" } else { "rejected" }.into(),
        );
        object.insert("resolved_at".into(), now.into());
        if let Some(resolved_by) = resolved_by {
            object.insert("resolved_by".into(), resolved_by.into());
        }
        if let Some(message) = &message {
            object.insert("message".into(), message.clone().into());
        }
    }
    let updated = db
        .update_automation_record("approval_requests".into(), approval_id, approval.clone())
        .await?;

    if let (Some(workflow_run_id), Some(node_id)) = (
        approval
            .get("workflow_run_id")
            .and_then(Value::as_str)
            .and_then(|raw| raw.parse::<Uuid>().ok()),
        approval.get("node_id").and_then(Value::as_str),
    ) {
        let node_runs = db.fetch_workflow_node_runs(workflow_run_id).await?;
        if let Some(node_run) = node_runs
            .iter()
            .filter(|run| run.node_id == node_id)
            .max_by_key(|run| run.created_at)
        {
            db.update_workflow_node_run(
                node_run.id,
                if approved {
                    WorkflowStatus::Succeeded
                } else {
                    WorkflowStatus::Blocked
                },
                None,
                None,
                Some(output_json.unwrap_or_else(|| {
                    runinator_models::json!({
                        "approval_id": approval_id,
                        "approved": approved
                    })
                })),
                Some(runinator_models::json!({
                    "approval_id": approval_id,
                    "approved": approved
                })),
                Some(if approved {
                    "approval_approved".into()
                } else {
                    "approval_rejected".into()
                }),
                message,
            )
            .await?;
        }
        db.update_workflow_run_status(
            workflow_run_id,
            if approved {
                WorkflowStatus::Running
            } else {
                WorkflowStatus::Blocked
            },
            Some(node_id.to_string()),
            None,
            None,
        )
        .await?;
        // wake the reducer so it re-processes the now-resolved approval node and transitions it.
        // the event-driven ready queue would otherwise never re-visit the parked node.
        if approved {
            support::enqueue_node_ready(
                db,
                workflow_run_id,
                node_id.to_string(),
                "approval_resolved",
                Utc::now(),
                runinator_models::json!({ "approval_id": approval_id }),
            )
            .await?;
        }
    }

    Ok(updated)
}

pub async fn fetch_gates<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Option<Uuid>,
    status: Option<String>,
) -> Result<Vec<Value>, SendableError> {
    db.fetch_gates(workflow_run_id, status).await
}

pub async fn fetch_gate<T: DatabaseImpl>(
    db: &T,
    gate_id: Uuid,
) -> Result<Option<Value>, SendableError> {
    db.fetch_gate(gate_id).await
}

pub async fn delete_gate<T: DatabaseImpl>(db: &T, gate_id: Uuid) -> Result<bool, SendableError> {
    db.delete_gate(gate_id).await
}

pub async fn delete_automation_record<T: DatabaseImpl>(
    db: &T,
    record_type: &str,
    record_id: Uuid,
) -> Result<bool, SendableError> {
    db.delete_automation_record(record_type.to_string(), record_id)
        .await
}

pub async fn create_gate<T: DatabaseImpl>(db: &T, record: Value) -> Result<Value, SendableError> {
    db.create_gate(record).await
}

/// open or close a gate from outside the reducer (manual ui action or an external system). on open
/// we wake the reducer so the parked gate node re-checks immediately instead of on the next poll.
pub async fn resolve_gate<T: DatabaseImpl>(
    db: &T,
    gate_id: Uuid,
    open: bool,
    reason: Option<String>,
    resolved_by: Option<String>,
) -> Result<Value, SendableError> {
    let Some(mut gate) = db.fetch_gate(gate_id).await? else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Gate {gate_id} not found"),
        )));
    };
    let now = Utc::now().timestamp();
    if let Some(object) = gate.as_object_mut() {
        object.insert("status".into(), if open { "open" } else { "closed" }.into());
        object.insert("resolved_at".into(), now.into());
        if let Some(resolved_by) = resolved_by {
            object.insert("resolved_by".into(), resolved_by.into());
        }
        if let Some(reason) = reason {
            object.insert("reason".into(), reason.into());
        }
    }
    let updated = db.update_gate(gate_id, gate.clone()).await?;

    if open {
        if let (Some(workflow_run_id), Some(node_id)) = (
            gate.get("workflow_run_id")
                .and_then(Value::as_str)
                .and_then(|raw| raw.parse::<Uuid>().ok()),
            gate.get("node_id").and_then(Value::as_str),
        ) {
            support::enqueue_node_ready(
                db,
                workflow_run_id,
                node_id.to_string(),
                "gate_opened",
                Utc::now(),
                runinator_models::json!({ "gate_id": gate_id }),
            )
            .await?;
        }
    }

    Ok(updated)
}
