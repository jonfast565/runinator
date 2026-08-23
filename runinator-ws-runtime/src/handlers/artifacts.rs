use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    body::Body,
    extract::{Multipart, Path, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_models::auth::AuthContext;
use runinator_models::runs::NewRunArtifact;
use runinator_store::{RuntimeStore, roles::TaskRunStore};

use runinator_engine::services::ArtifactOperations;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, RequestDoc, endpoint, json_body};
use runinator_ws_core::responses::{api_error, bad_request};
use runinator_ws_middleware::authz::AuthContextExt;

pub async fn get_run_artifacts<T: TaskRunStore + RuntimeStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(artifacts): Extension<Arc<ArtifactOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match artifacts.list_for_run(run_id).await {
        Ok(artifacts) => (StatusCode::OK, Json(ApiResponse::RunArtifacts(artifacts))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn add_run_artifact<T: TaskRunStore + RuntimeStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(artifacts): Extension<Arc<ArtifactOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(run_id): Path<Uuid>,
    Json(artifact): Json<NewRunArtifact>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match artifacts.add(run_id, &artifact).await {
        Ok(artifact) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::RunArtifacts(vec![artifact])),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_artifacts<T: TaskRunStore + RuntimeStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(artifacts): Extension<Arc<ArtifactOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match artifacts.list().await {
        Ok(artifacts) => (StatusCode::OK, Json(ApiResponse::RunArtifacts(artifacts))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn upload_artifact<T: TaskRunStore + RuntimeStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(artifacts): Extension<Arc<ArtifactOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    mut multipart: Multipart,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    let mut run_id: Option<Uuid> = None;
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

    match artifacts
        .persist(run_id, &resolved_name, &resolved_mime, &bytes, ctx.org_id)
        .await
    {
        Ok(artifact) => (
            StatusCode::OK,
            Json(ApiResponse::RunArtifacts(vec![artifact])),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_artifact<T: TaskRunStore + RuntimeStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(artifacts): Extension<Arc<ArtifactOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(artifact_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match artifacts.delete(artifact_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(
                runinator_models::web::TaskResponse {
                    success: true,
                    message: "Artifact deleted".to_string(),
                },
            )),
        ),
        Ok(false) => {
            runinator_ws_core::responses::not_found(format!("Artifact {artifact_id} not found"))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn download_artifact<T: TaskRunStore + RuntimeStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(artifacts): Extension<Arc<ArtifactOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(artifact_id): Path<Uuid>,
) -> Response {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply.into_response();
    }
    let artifact = match artifacts.fetch(artifact_id).await {
        Ok(Some(artifact)) => artifact,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "artifact not found").into_response();
        }
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };

    let content = match artifacts.open(&artifact.uri).await {
        Ok(content) => content,
        Err(err) => {
            return (
                StatusCode::NOT_FOUND,
                format!("artifact bytes unavailable: {err}"),
            )
                .into_response();
        }
    };
    // the stored length wins over the row's, which a pre-blob upload could disagree with after a
    // manual edit; a wrong content-length truncates the download rather than erroring.
    let length = content.size_bytes;
    let stream = tokio_util::io::ReaderStream::new(content.body);
    let body = Body::from_stream(stream);
    let disposition = format!(
        "attachment; filename=\"{}\"",
        artifact.name.replace('"', "")
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &artifact.mime_type)
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CONTENT_LENGTH, length)
        .body(body)
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "response build failed").into_response()
        })
}

/// the query a caller uses to describe the bytes it is sending to `/artifacts/content`.
#[derive(serde::Deserialize)]
pub struct ArtifactContentQuery {
    pub run_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// `POST /artifacts/content` — store artifact bytes and return the URI to record against them.
///
/// deliberately writes no database row. a worker-produced artifact already gets its row from the
/// result-event path, so an endpoint that also inserted one would produce two rows for one artifact;
/// this splits the byte transfer from the bookkeeping instead.
///
/// the body is the raw bytes rather than a multipart form: the caller is a worker, not a browser,
/// and multipart would buy nothing but a parser.
pub async fn upload_artifact_content<T: TaskRunStore + RuntimeStore>(
    Extension(artifacts): Extension<Arc<ArtifactOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<ArtifactContentQuery>,
    body: axum::body::Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    let bytes = body.to_vec();
    let resolved_name = query.name.unwrap_or_else(|| "artifact".to_string());
    let resolved_mime = query.mime_type.unwrap_or_else(|| {
        mime_guess::from_path(&resolved_name)
            .first_or_octet_stream()
            .essence_str()
            .to_string()
    });

    match artifacts
        .put_content(query.run_id, &resolved_name, &resolved_mime, &bytes)
        .await
    {
        Ok(uri) => (
            StatusCode::CREATED,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "uri": uri,
                "size_bytes": bytes.len() as i64,
                "sha256": runinator_blob_core::sha256_hex(&bytes),
            }))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// the `artifacts` endpoints.
pub fn routes<T: TaskRunStore + RuntimeStore>(pool: std::sync::Arc<T>) -> axum::Router {
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
            "/artifacts/content",
            post(upload_artifact_content::<T>).layer(Extension(pool.clone())),
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
pub const DOCS: &[EndpointDoc] = &[
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
        "post",
        "/artifacts/content",
        "Artifacts",
        "Store artifact bytes",
        "Stores artifact content in the object store and returns its uri, size, and sha-256. Records no artifact row: callers whose artifact is already recorded by the worker result path use this to move the bytes without creating a second row.",
        false,
        Some(RequestDoc {
            description: "Raw artifact bytes.",
            example: Example::Artifact,
            content_type: "application/octet-stream",
        }),
        &[],
        201,
        "artifact bytes stored",
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
