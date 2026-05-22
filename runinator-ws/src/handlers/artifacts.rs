use std::sync::Arc;

use axum::{
    Extension, Json,
    body::Body,
    extract::{Multipart, Path},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::runs::NewRunArtifact;

use crate::events::{AppEvent, EventSender, emit};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, bad_request};

pub(crate) async fn get_run_artifacts<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_run_artifacts(db.as_ref(), run_id).await {
        Ok(artifacts) => (StatusCode::OK, Json(ApiResponse::RunArtifacts(artifacts))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn add_run_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(run_id): Path<i64>,
    Json(artifact): Json<NewRunArtifact>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::add_run_artifact(db.as_ref(), run_id, &artifact).await {
        Ok(artifact) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::RunArtifacts(vec![artifact])),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn list_artifacts<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_all_artifacts(db.as_ref()).await {
        Ok(artifacts) => (StatusCode::OK, Json(ApiResponse::RunArtifacts(artifacts))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn upload_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    mut multipart: Multipart,
) -> (StatusCode, Json<ApiResponse>) {
    let mut run_id: Option<i64> = None;
    let mut node_run_id: Option<i64> = None;
    let mut name: Option<String> = None;
    let mut mime_type: Option<String> = None;
    let mut file_name: Option<String> = None;
    let mut bytes: Vec<u8> = Vec::new();
    let mut has_file = false;

    while let Some(field_result) = match multipart.next_field().await {
        Ok(value) => value.map(Ok),
        Err(err) => Some(Err(err)),
    } {
        let mut field = match field_result {
            Ok(field) => field,
            Err(err) => return bad_request(format!("multipart error: {err}")),
        };
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "run_id" => {
                let raw = field.text().await.unwrap_or_default();
                run_id = raw.parse().ok();
            }
            "workflow_node_run_id" => {
                let raw = field.text().await.unwrap_or_default();
                node_run_id = raw.parse().ok();
            }
            "name" => {
                name = Some(field.text().await.unwrap_or_default());
            }
            "mime_type" => {
                mime_type = Some(field.text().await.unwrap_or_default());
            }
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                let chunk_name = field_name.clone();
                while let Some(chunk) = match field.chunk().await {
                    Ok(value) => value.map(Ok),
                    Err(err) => Some(Err(err)),
                } {
                    let chunk = match chunk {
                        Ok(bytes) => bytes,
                        Err(err) => {
                            return bad_request(format!("multipart error in {chunk_name}: {err}"));
                        }
                    };
                    bytes.extend_from_slice(&chunk);
                }
                has_file = true;
            }
            _ => {
                // unknown field; ignore.
                let _ = field.text().await;
            }
        }
    }

    let Some(run_id) = run_id else {
        return bad_request("missing run_id".to_string());
    };
    if !has_file {
        return bad_request("missing file part".to_string());
    }
    let resolved_name = name
        .or(file_name.clone())
        .unwrap_or_else(|| "artifact".to_string());
    let resolved_mime = mime_type.unwrap_or_else(|| {
        mime_guess::from_path(&resolved_name)
            .first_or_octet_stream()
            .essence_str()
            .to_string()
    });

    match repository::persist_artifact_file(
        db.as_ref(),
        run_id,
        node_run_id,
        &resolved_name,
        &resolved_mime,
        &bytes,
    )
    .await
    {
        Ok(artifact) => {
            emit(
                &events,
                AppEvent::ArtifactCreated {
                    artifact_id: artifact.id,
                    run_id: artifact.run_id,
                },
            );
            (
                StatusCode::OK,
                Json(ApiResponse::RunArtifacts(vec![artifact])),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn download_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(artifact_id): Path<i64>,
) -> Response {
    let artifact = match db.fetch_artifact(artifact_id).await {
        Ok(Some(artifact)) => artifact,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "artifact not found").into_response();
        }
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };

    let path = std::path::PathBuf::from(&artifact.uri);
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(err) => {
            return (
                StatusCode::NOT_FOUND,
                format!("artifact file missing at {}: {}", path.display(), err),
            )
                .into_response();
        }
    };
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);
    let disposition = format!(
        "attachment; filename=\"{}\"",
        artifact.name.replace('"', "")
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &artifact.mime_type)
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CONTENT_LENGTH, artifact.size_bytes)
        .body(body)
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "response build failed").into_response()
        })
}
