use runinator_store::RuntimeStore;
use uuid::Uuid;

/// Resolve the owning org frozen into a workflow run. A live ownership transfer applies to future
/// admissions; it must not move an already-admitted run into another tenant's coordination space.
pub async fn org_id_for_workflow_run<T: RuntimeStore>(
    db: &T,
    workflow_run_id: Uuid,
) -> Option<Uuid> {
    let run = db
        .fetch_workflow_run(workflow_run_id)
        .await
        .ok()
        .flatten()?;
    if let Some(snapshot) = run.workflow_snapshot {
        return snapshot.org_id;
    }
    db.fetch_workflow(run.workflow_id)
        .await
        .ok()
        .flatten()
        .and_then(|workflow| workflow.org_id)
}

/// Resolve the owning org frozen into a pipeline run, with the live row only as legacy fallback.
pub async fn org_id_for_pipeline_run<T: RuntimeStore>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Option<Uuid> {
    let run = db
        .fetch_pipeline_run(pipeline_run_id)
        .await
        .ok()
        .flatten()?;
    if let Some(snapshot) = run.pipeline_snapshot {
        return snapshot.org_id;
    }
    db.fetch_pipeline(run.pipeline_id)
        .await
        .ok()
        .flatten()
        .and_then(|pipeline| pipeline.org_id)
}
