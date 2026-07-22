use runinator_database::interfaces::DatabaseImpl;
use uuid::Uuid;

/// resolve the owning org for a workflow run. prefers the live workflow row so an org reassignment
/// is reflected; falls back to the run snapshot when the workflow row is gone.
pub async fn org_id_for_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Option<Uuid> {
    let run = db.fetch_workflow_run(workflow_run_id).await.ok().flatten()?;
    if let Ok(Some(workflow)) = db.fetch_workflow(run.workflow_id).await {
        if workflow.org_id.is_some() {
            return workflow.org_id;
        }
    }
    run.workflow_snapshot.and_then(|snapshot| snapshot.org_id)
}

/// resolve the owning org for a pipeline run via the live pipeline row, then the snapshot.
pub async fn org_id_for_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Option<Uuid> {
    let run = db.fetch_pipeline_run(pipeline_run_id).await.ok().flatten()?;
    if let Ok(Some(pipeline)) = db.fetch_pipeline(run.pipeline_id).await {
        if pipeline.org_id.is_some() {
            return pipeline.org_id;
        }
    }
    run.pipeline_snapshot.and_then(|snapshot| snapshot.org_id)
}
