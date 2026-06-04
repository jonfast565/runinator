use super::*;

pub(super) async fn fetch_workflow_snapshot<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<WorkflowDefinition, SendableError> {
    db.fetch_workflow(workflow_id).await?.ok_or_else(|| {
        Box::new(RuntimeError::new(
            "workflow.not_found".into(),
            format!("Workflow {workflow_id} not found"),
        )) as SendableError
    })
}

pub(super) async fn enqueue_start_ready_node<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
) -> Result<(), SendableError> {
    let workflow = run.workflow_snapshot.as_ref().ok_or_else(|| {
        Box::new(RuntimeError::new(
            "workflow_run.snapshot_missing".into(),
            format!("Workflow run {} is missing its workflow snapshot", run.id),
        )) as SendableError
    })?;
    let (start, _) = runinator_workflows::parse_nodes(workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let event = NewOrchestrationEvent::new(
        run.id,
        Some(start.clone()),
        "workflow_run_created",
        runinator_models::json!({
            "workflow_id": run.workflow_id,
            "node_id": start.clone(),
        }),
    );
    db.enqueue_ready_node(event, start, Utc::now()).await?;
    Ok(())
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

pub(super) async fn enqueue_node_ready<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
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
