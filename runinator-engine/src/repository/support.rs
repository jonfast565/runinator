use super::*;
use uuid::Uuid;

pub(super) async fn fetch_workflow_snapshot<T: DatabaseImpl>(
    db: &T,
    workflow_id: Uuid,
) -> Result<WorkflowDefinition, SendableError> {
    db.fetch_workflow(workflow_id)
        .await?
        .ok_or_else(|| runinator_runtime::errors::WORKFLOW_NOT_FOUND.error(workflow_id))
}

pub(super) fn normalized_run_name(name: Option<String>) -> Option<String> {
    name.and_then(|value| {
        let stripped = value.trim().to_string();
        if stripped.is_empty() {
            None
        } else {
            Some(stripped)
        }
    })
}

/// arm a wake addressed to one thread of control.
///
/// the debug endpoints historically enqueued nothing at all, which is half of why the debugger never
/// resumed anything. stamping the cursor is what lets a step revive one branch of a fan-out without
/// superseding a sibling's pending wake.
pub(super) async fn enqueue_node_ready_for_cursor<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    node_id: String,
    event_type: &str,
    ready_at: chrono::DateTime<Utc>,
) -> Result<(), SendableError> {
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node_id.clone()),
        event_type.to_string(),
        runinator_models::json!({ "cursor_id": cursor_id }),
    )
    .for_cursor(cursor_id);
    db.enqueue_ready_node(event, node_id, ready_at).await?;
    Ok(())
}

pub(super) async fn enqueue_node_ready<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node_id: String,
    event_type: &str,
    ready_at: chrono::DateTime<Utc>,
    payload: Value,
) -> Result<(), SendableError> {
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node_id.clone()),
        event_type.to_string(),
        payload,
    );
    db.enqueue_ready_node(event, node_id, ready_at).await?;
    Ok(())
}
