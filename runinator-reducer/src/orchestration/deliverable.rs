use super::context::runtime_context;
use super::transitions::transition_from_node;
use super::*;
use crate::errors::DELIVERABLE_SOURCE_UNRESOLVED;
use runinator_models::json;
use runinator_models::workflows::NewWorkflowRunDeliverable;

// a deliverable node names upstream artifacts and promotes them to run-level deliverables, so a
// finished run exposes what it produced. each declared item resolves to one or more artifact
// descriptors (the same shape `steps.<node>.artifacts` carries) and becomes a deliverable row.
pub(super) async fn process_deliverable_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let params = runinator_workflows::parse_deliverable_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let context = runtime_context(db, workflow_run, node_runs).await;

    let mut manifest = Vec::new();
    for item in &params.items {
        let resolved = runinator_workflows::resolve_value_refs(&item.source, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        for artifact in artifact_values(&resolved) {
            let deliverable = build_deliverable(workflow_run.id, &node.id, &item.name, artifact)?;
            let stored = db.add_workflow_run_deliverable(&deliverable).await?;
            manifest.push(json!({
                "id": stored.id,
                "name": stored.name,
                "artifact_id": stored.artifact_id,
                "mime_type": stored.mime_type,
                "size_bytes": stored.size_bytes,
                "uri": stored.uri,
            }));
        }
    }

    let output = json!({ "deliverables": manifest });
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output),
        Some("deliverables_recorded".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

// flatten a resolved source into the artifact descriptors it carries (single object or array).
fn artifact_values(resolved: &Value) -> Vec<&Value> {
    match resolved {
        Value::Array(items) => items.iter().collect(),
        Value::Null => Vec::new(),
        other => vec![other],
    }
}

fn build_deliverable(
    workflow_run_id: Uuid,
    node_id: &str,
    name: &str,
    artifact: &Value,
) -> Result<NewWorkflowRunDeliverable, SendableError> {
    let object = artifact.as_object().ok_or_else(|| {
        DELIVERABLE_SOURCE_UNRESOLVED.error(format!("'{name}' is not an artifact"))
    })?;
    let artifact_id = object
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| {
            DELIVERABLE_SOURCE_UNRESOLVED.error(format!("'{name}' has no artifact id"))
        })?;
    let uri = object
        .get("uri")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| DELIVERABLE_SOURCE_UNRESOLVED.error(format!("'{name}' has no uri")))?;
    let mime_type = object
        .get("mime_type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();
    let size_bytes = object
        .get("size_bytes")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let metadata = object.get("metadata").cloned().unwrap_or(Value::Null);
    Ok(NewWorkflowRunDeliverable {
        workflow_run_id,
        node_id: node_id.to_string(),
        artifact_id,
        name: name.to_string(),
        mime_type,
        size_bytes,
        uri,
        metadata,
    })
}
