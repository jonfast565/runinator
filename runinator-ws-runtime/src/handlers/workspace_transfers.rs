//! Authorized durable archive jobs.
use super::workspaces::failed;
use axum::{
    Extension, Json,
    body::Body,
    extract::{DefaultBodyLimit, Path},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_engine::services::WorkspaceService;
use runinator_models::{
    auth::{AuthContext, Permission, ResourceType},
    workspaces::*,
};
use runinator_store::DatabaseImpl;
use runinator_ws_core::ValidatedJson;
use runinator_ws_middleware::authz::{AuthzChecker, IntoReply};
use std::sync::Arc;
use uuid::Uuid;
#[derive(serde::Deserialize)]
pub struct Create {
    #[serde(default)]
    pub filesystem: bool,
    pub version: i64,
    #[serde(default)]
    pub importing: bool,
}
pub async fn create<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<Create>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(
            ResourceType::Workspace,
            id,
            if request.importing {
                Permission::Edit
            } else {
                Permission::View
            },
        )
        .await
    {
        return reply.into_reply().into_response();
    }
    match service
        .create_transfer(id, request.version, request.importing, request.filesystem)
        .await
    {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => failed(error),
    }
}
async fn authorize<T: DatabaseImpl>(
    db: &T,
    service: &WorkspaceService<T>,
    ctx: &AuthContext,
    id: Uuid,
    edit: bool,
) -> Result<WorkspaceTransfer, Response> {
    let job = service.transfer(id).await.map_err(failed)?;
    AuthzChecker::new(db, ctx)
        .require_resource(
            ResourceType::Workspace,
            job.workspace_id,
            if edit || job.importing {
                Permission::Edit
            } else {
                Permission::View
            },
        )
        .await
        .map_err(|reply| reply.into_reply().into_response())?;
    Ok(job)
}
pub async fn get<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> Response {
    match authorize(db.as_ref(), &service, &ctx, id, false).await {
        Ok(job) => Json(job).into_response(),
        Err(reply) => reply,
    }
}
pub async fn cancel<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(reply) = authorize(db.as_ref(), &service, &ctx, id, true).await {
        return reply;
    }
    match service.cancel_transfer(id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failed(error),
    }
}
pub async fn upload<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    body: Body,
) -> Response {
    if let Err(reply) = authorize(db.as_ref(), &service, &ctx, id, true).await {
        return reply;
    }
    use futures_util::TryStreamExt;
    let reader =
        tokio_util::io::StreamReader::new(body.into_data_stream().map_err(std::io::Error::other));
    match service.upload_transfer(id, reader).await {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => failed(error),
    }
}
pub async fn content<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(reply) = authorize(db.as_ref(), &service, &ctx, id, false).await {
        return reply;
    }
    match service.transfer_content(id).await {
        Ok(content) => (
            [
                (
                    header::CONTENT_TYPE,
                    "application/vnd.oci.image.layout.v1+tar".to_string(),
                ),
                (header::CONTENT_LENGTH, content.size_bytes.to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=workspace.oci.tar".into(),
                ),
            ],
            Body::from_stream(tokio_util::io::ReaderStream::new(content.body)),
        )
            .into_response(),
        Err(error) => failed(error),
    }
}
pub fn routes<T: DatabaseImpl>() -> axum::Router {
    axum::Router::new()
        .route(
            "/workspaces/{id}/transfers",
            axum::routing::post(create::<T>),
        )
        .route(
            "/workspace-transfers/{id}",
            axum::routing::get(get::<T>).delete(cancel::<T>),
        )
        .route(
            "/workspace-transfers/{id}/content",
            axum::routing::get(content::<T>)
                .put(upload::<T>)
                .layer(DefaultBodyLimit::disable()),
        )
}

impl runinator_models::validation::Validate for Create {
    fn validate(&self) -> Result<(), runinator_models::validation::ValidationError> {
        if (self.importing && self.version != 0) || (!self.importing && self.version <= 0) {
            return Err(runinator_models::validation::ValidationError::new(
                "version",
                "imports require version zero; exports require a retained positive version",
            ));
        }
        if self.importing && self.filesystem {
            return Err(runinator_models::validation::ValidationError::new(
                "filesystem",
                "applies only to exports",
            ));
        }
        Ok(())
    }
}
