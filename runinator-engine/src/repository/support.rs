use super::*;
use uuid::Uuid;

pub(super) async fn fetch_workflow_snapshot<T: RuntimeStore>(
    db: &T,
    workflow_id: Uuid,
) -> Result<WorkflowDefinition, SendableError> {
    db.fetch_workflow(workflow_id)
        .await?
        .ok_or_else(|| runinator_runtime::errors::WORKFLOW_NOT_FOUND.error(workflow_id))
}

/// Load the immutable definition named by a durable revision pin. The mutable workflow row only
/// supplies logical identity (namespace/org/enabled); executable graph data comes from the
/// revision snapshot, so a child run started after a later deploy still runs exactly what its
/// parent recorded.
pub(crate) async fn fetch_workflow_revision_snapshot<T: RuntimeStore + DefinitionStore>(
    db: &T,
    workflow_id: Uuid,
    revision: i64,
    digest: &str,
) -> Result<WorkflowDefinition, SendableError> {
    let current = fetch_workflow_snapshot(db, workflow_id).await?;
    let stored = db
        .fetch_workflow_revision(workflow_id, revision)
        .await?
        .ok_or_else(|| {
            runinator_runtime::errors::WORKFLOW_NOT_FOUND
                .error(format!("workflow {workflow_id} has no revision {revision}"))
        })?;
    if stored.digest != digest {
        return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
            "workflow {workflow_id} revision {revision} digest does not match its pin"
        )));
    }
    Ok(stored.to_definition(&current))
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
