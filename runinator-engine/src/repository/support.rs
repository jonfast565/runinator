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
