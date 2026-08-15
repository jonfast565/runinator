use std::sync::Arc;

use runinator_blob_core::BlobStore;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    web::TaskResponse,
};
use uuid::Uuid;

pub async fn fetch_run_chunks<T: DatabaseImpl>(
    db: &T,
    run_id: Uuid,
    cursor: Option<i64>,
    limit: i64,
) -> Result<Vec<RunChunk>, SendableError> {
    db.fetch_run_chunks(run_id, cursor, limit).await
}

pub async fn fetch_runs_by_status<T: DatabaseImpl>(
    db: &T,
    status: RunStatus,
) -> Result<Vec<RunSummary>, SendableError> {
    db.fetch_runs_by_status(status).await
}

pub async fn update_run_status<T: DatabaseImpl>(
    db: &T,
    run_id: Uuid,
    status: RunStatus,
    output_json: Option<Value>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_run_status(run_id, status, output_json, message)
        .await?;
    Ok(TaskResponse {
        success: true,
        message: "Run updated".into(),
    })
}

pub async fn append_run_chunk<T: DatabaseImpl>(
    db: &T,
    run_id: Uuid,
    chunk: &NewRunChunk,
) -> Result<RunChunk, SendableError> {
    db.append_run_chunk(run_id, chunk).await
}

pub async fn fetch_run_artifacts<T: DatabaseImpl>(
    db: &T,
    run_id: Uuid,
) -> Result<Vec<RunArtifact>, SendableError> {
    db.fetch_run_artifacts(run_id).await
}

pub async fn add_run_artifact<T: DatabaseImpl>(
    db: &T,
    run_id: Uuid,
    artifact: &NewRunArtifact,
) -> Result<RunArtifact, SendableError> {
    db.add_run_artifact(run_id, artifact).await
}

pub async fn fetch_all_artifacts<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<RunArtifact>, SendableError> {
    db.fetch_all_artifacts().await
}

pub async fn fetch_artifact<T: DatabaseImpl>(
    db: &T,
    artifact_id: Uuid,
) -> Result<Option<RunArtifact>, SendableError> {
    db.fetch_artifact(artifact_id).await
}

/// delete an artifact: remove its bytes (best-effort) then remove the db row. returns false when no
/// such artifact exists.
pub async fn delete_artifact<T: DatabaseImpl>(
    db: &T,
    blobs: &Arc<dyn BlobStore>,
    artifact_id: Uuid,
) -> Result<bool, SendableError> {
    let Some(artifact) = db.fetch_artifact(artifact_id).await? else {
        return Ok(false);
    };
    // bytes first; a missing object should not block the row delete.
    crate::artifact_storage::delete_artifact_bytes(blobs, &artifact.uri).await;
    db.delete_artifact(artifact_id).await
}

/// store uploaded artifact bytes and record the row(s) that point at them.
pub async fn persist_artifact_file<T: DatabaseImpl>(
    db: &T,
    blobs: &Arc<dyn BlobStore>,
    run_id: Uuid,
    workflow_node_run_id: Option<Uuid>,
    name: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<RunArtifact, SendableError> {
    let uri = crate::artifact_storage::put_artifact(
        blobs,
        run_id,
        workflow_node_run_id,
        name,
        mime_type,
        bytes,
    )
    .await?;
    let new_artifact = NewRunArtifact {
        name: name.to_string(),
        mime_type: mime_type.to_string(),
        size_bytes: bytes.len() as i64,
        uri: uri.clone(),
        metadata: runinator_models::json!({
            "source": "upload",
            "workflow_node_run_id": workflow_node_run_id
        }),
    };
    let artifact = db.add_run_artifact(run_id, &new_artifact).await?;

    if let Some(node_run_id) = workflow_node_run_id {
        let _ = db
            .add_workflow_node_run_artifact(node_run_id, &new_artifact)
            .await;
    }

    Ok(artifact)
}
