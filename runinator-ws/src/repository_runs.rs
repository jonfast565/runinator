use runinator_database::interfaces::DatabaseImpl;
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    web::TaskResponse,
};

pub async fn fetch_run_chunks<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
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
    run_id: i64,
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
    run_id: i64,
    chunk: &NewRunChunk,
) -> Result<RunChunk, SendableError> {
    db.append_run_chunk(run_id, chunk).await
}

pub async fn fetch_run_artifacts<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
) -> Result<Vec<RunArtifact>, SendableError> {
    db.fetch_run_artifacts(run_id).await
}

pub async fn add_run_artifact<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
    artifact: &NewRunArtifact,
) -> Result<RunArtifact, SendableError> {
    db.add_run_artifact(run_id, artifact).await
}

pub async fn fetch_all_artifacts<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<RunArtifact>, SendableError> {
    db.fetch_all_artifacts().await
}

pub async fn persist_artifact_file<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
    workflow_node_run_id: Option<i64>,
    name: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<RunArtifact, SendableError> {
    use runinator_utilities::app_data;

    let safe_name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe_name = if safe_name.is_empty() {
        "artifact".to_string()
    } else {
        safe_name
    };

    let dir = app_data::app_data_path(format!("artifacts/{run_id}"))?;
    tokio::fs::create_dir_all(&dir).await?;
    let id_suffix = uuid::Uuid::new_v4().simple().to_string();
    let final_name = format!("{}-{}", id_suffix, safe_name);
    let target = dir.join(&final_name);
    tokio::fs::write(&target, bytes).await?;

    let uri = target.to_string_lossy().to_string();
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
