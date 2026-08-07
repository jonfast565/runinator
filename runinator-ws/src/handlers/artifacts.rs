use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    body::Body,
    extract::{Multipart, Path},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;
use runinator_models::runs::NewRunArtifact;

use crate::events::{AppEvent, AppEventKind, EventSender, emit};
use crate::models::ApiResponse;
use crate::openapi::docs::{EndpointDoc, Example, RequestDoc, endpoint, json_body};
use crate::repository;
use crate::responses::{api_error, bad_request};

pub(crate) async fn get_run_artifacts<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::fetch_run_artifacts(db.as_ref(), run_id).await {
        Ok(artifacts) => (StatusCode::OK, Json(ApiResponse::RunArtifacts(artifacts))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn add_run_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(run_id): Path<Uuid>,
    Json(artifact): Json<NewRunArtifact>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
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
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::fetch_all_artifacts(db.as_ref()).await {
        Ok(artifacts) => (StatusCode::OK, Json(ApiResponse::RunArtifacts(artifacts))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn upload_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    mut multipart: Multipart,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    let mut run_id: Option<Uuid> = None;
    let mut node_run_id: Option<Uuid> = None;
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
            let org_id = if let Some(node_run_id) = node_run_id {
                match repository::fetch_workflow_node_run(db.as_ref(), node_run_id).await {
                    Ok(Some(node_run)) => {
                        repository::org_id_for_workflow_run(db.as_ref(), node_run.workflow_run_id)
                            .await
                            .or(ctx.org_id)
                    }
                    _ => ctx.org_id,
                }
            } else {
                ctx.org_id
            };
            emit(
                &events,
                AppEvent::new(
                    org_id,
                    AppEventKind::ArtifactCreated {
                        artifact_id: artifact.id,
                        run_id: artifact.run_id,
                    },
                ),
            );
            (
                StatusCode::OK,
                Json(ApiResponse::RunArtifacts(vec![artifact])),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(artifact_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::delete_artifact(db.as_ref(), artifact_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(
                runinator_models::web::TaskResponse {
                    success: true,
                    message: "Artifact deleted".to_string(),
                },
            )),
        ),
        Ok(false) => crate::responses::not_found(format!("Artifact {artifact_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn download_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(artifact_id): Path<Uuid>,
) -> Response {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply.into_response();
    }
    let artifact = match repository::fetch_artifact(db.as_ref(), artifact_id).await {
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

/// the `artifacts` endpoints.
pub(crate) fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route(
            "/runs/{id}/artifacts",
            get(get_run_artifacts::<T>)
                .post(add_run_artifact::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_ARTIFACTS,
            get(list_artifacts::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/artifacts/upload",
            post(upload_artifact::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/artifacts/{id}/download",
            get(download_artifact::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/artifacts/{id}",
            delete(delete_artifact::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub(crate) const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/runs/{id}/artifacts",
        "Artifacts",
        "List task run artifacts",
        "Lists artifacts linked to a low-level task run.",
        false,
        None,
        &[],
        200,
        "run artifacts",
        Example::Artifact,
    ),
    endpoint(
        "post",
        "/runs/{id}/artifacts",
        "Artifacts",
        "Attach a task run artifact",
        "Registers an artifact produced by a low-level task run.",
        false,
        json_body("Artifact metadata to attach.", Example::Artifact),
        &[],
        202,
        "run artifact attached",
        Example::Artifact,
    ),
    endpoint(
        "get",
        "/artifacts",
        "Artifacts",
        "List artifacts",
        "Lists stored artifacts across runs when permitted by the caller.",
        false,
        None,
        &[],
        200,
        "artifacts",
        Example::Artifact,
    ),
    endpoint(
        "post",
        "/artifacts/upload",
        "Artifacts",
        "Upload artifact bytes",
        "Uploads artifact content as multipart form data and records artifact metadata.",
        false,
        Some(RequestDoc {
            description: "Multipart artifact upload payload.",
            example: Example::Artifact,
            content_type: "multipart/form-data",
        }),
        &[],
        200,
        "artifact uploaded",
        Example::Artifact,
    ),
    endpoint(
        "get",
        "/artifacts/{id}/download",
        "Artifacts",
        "Download an artifact",
        "Downloads artifact bytes for the requested artifact id.",
        false,
        None,
        &[],
        200,
        "artifact bytes",
        Example::None,
    ),
];
