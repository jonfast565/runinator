use super::*;
use uuid::Uuid;

pub async fn upsert_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline: &Pipeline,
) -> Result<Pipeline, SendableError> {
    db.upsert_pipeline(pipeline).await
}

pub async fn fetch_pipelines<T: DatabaseImpl>(db: &T) -> Result<Vec<Pipeline>, SendableError> {
    db.fetch_pipelines().await
}

pub async fn fetch_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<Option<Pipeline>, SendableError> {
    db.fetch_pipeline(pipeline_id).await
}

pub async fn delete_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_pipeline(pipeline_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Pipeline deleted".into(),
    })
}

pub async fn set_pipeline_org<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
    org_id: Option<Uuid>,
) -> Result<(), SendableError> {
    db.set_pipeline_org(pipeline_id, org_id).await
}
