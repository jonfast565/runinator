use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_engine::services::WorkflowFiles;
use runinator_models::auth::AuthContext;

use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, RequestDoc, endpoint};
use runinator_ws_core::responses::api_error;
use runinator_ws_middleware::authz::AuthContextExt;

/// Metadata accompanying a worker's artifact-byte upload.
#[derive(serde::Deserialize)]
pub struct ArtifactContentQuery {
    pub run_id: uuid::Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// `POST /artifacts/content` stores bytes only. VM effect-result processing persists the metadata,
/// so this endpoint never creates a second database record.
pub async fn upload_artifact_content<T: Send + Sync + 'static>(
    Extension(files): Extension<Arc<WorkflowFiles<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<ArtifactContentQuery>,
    body: axum::body::Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply.into_reply();
    }

    let bytes = body.to_vec();
    let name = query.name.unwrap_or_else(|| "artifact".to_string());
    let mime_type = query.mime_type.unwrap_or_else(|| {
        mime_guess::from_path(&name)
            .first_or_octet_stream()
            .essence_str()
            .to_string()
    });

    match files
        .put_artifact(query.run_id, &name, &mime_type, &bytes)
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

pub fn routes<T: Send + Sync + 'static>() -> axum::Router {
    axum::Router::new().route(
        runinator_models::api_routes::API_ARTIFACTS_CONTENT,
        axum::routing::post(upload_artifact_content::<T>),
    )
}

pub const DOCS: &[EndpointDoc] = &[endpoint!(
    "post",
    "/artifacts/content",
    "Artifacts",
    "Store artifact bytes",
    "Stores artifact content in the object store and returns its uri, size, and sha-256. VM effect-result processing records the artifact metadata separately.",
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
)];
