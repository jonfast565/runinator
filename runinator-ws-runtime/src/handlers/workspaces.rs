//! Durable workspace management and assigned-worker content transfer.
use axum::{
    Extension, Json,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_engine::services::WorkspaceService;
use runinator_models::{
    auth::{AuthContext, Permission, PrincipalKind, ResourceType},
    rbac::{Action, ResourceOwnership, ScopeKind, ScopeRef, SystemRole},
    workspaces::*,
};
use runinator_store::DatabaseImpl;
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint};
use runinator_ws_middleware::authz::{AuthContextExt, AuthzChecker, IntoReply};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct Page {
    #[serde(default = "page_size")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}
fn page_size() -> i64 {
    50
}
#[derive(Deserialize)]
pub struct Create {
    key: String,
}
impl runinator_models::validation::Validate for Create {
    fn validate(&self) -> Result<(), runinator_models::validation::ValidationError> {
        runinator_models::validation::required_text("key", &self.key, 200)
    }
}
#[derive(Deserialize)]
pub struct FileQuery {
    path: Option<String>,
}
#[derive(Deserialize)]
pub struct WorkerQuery {
    replica_id: Uuid,
}
fn failed(error: impl std::fmt::Display) -> Response {
    (StatusCode::CONFLICT, error.to_string()).into_response()
}

pub async fn list<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(page): Query<Page>,
) -> Response {
    let items = match service.list(ctx.org_id, page.limit, page.offset).await {
        Ok(items) => items,
        Err(error) => return failed(error),
    };
    let checker = AuthzChecker::new(db.as_ref(), &ctx);
    let mut visible = Vec::new();
    for workspace in items {
        match checker
            .resource_permission(ResourceType::Workspace, workspace.id)
            .await
        {
            Ok(Some(permission)) => visible.push(WorkspaceView {
                workspace,
                permission,
            }),
            Ok(None) => {}
            Err(reply) => return reply.into_response(),
        }
    }
    Json(visible).into_response()
}

pub async fn create<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<Create>,
) -> Response {
    if let Err(reply) = ctx.require_scope_action(Action::Edit, ctx.selected_scope()) {
        return reply.into_reply().into_response();
    }
    let now = chrono::Utc::now();
    let id = Uuid::now_v7();
    let tenant = ctx.selected_scope();
    let owner = if tenant.kind == ScopeKind::Platform {
        ScopeRef::PLATFORM
    } else if ctx.kind == PrincipalKind::User {
        ScopeRef {
            kind: ScopeKind::User,
            id: ctx.principal_id,
        }
    } else {
        tenant
    };
    let workspace = DurableWorkspace {
        id,
        key: request.key,
        org_id: ctx.org_id,
        head_version: 0,
        revision: 1,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    };
    let ownership = ResourceOwnership {
        resource_type: ResourceType::Workspace,
        resource_id: id,
        tenant,
        owner,
        created_by: ctx.principal_id,
        authz_version: 1,
        created_at: now,
        updated_at: now,
    };
    match service.create(workspace, ownership).await {
        Ok(item) => {
            if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                .require_resource(ResourceType::Workspace, item.id, Permission::Edit)
                .await
            {
                return reply.into_reply().into_response();
            }
            (StatusCode::CREATED, Json(item)).into_response()
        }
        Err(error) => failed(error),
    }
}

pub async fn detail<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service.get(id).await {
        Ok(Some(item)) => Json(item).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => failed(error),
    }
}

pub async fn versions<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Query(page): Query<Page>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service.versions(id, page.limit, page.offset).await {
        Ok(items) => Json(items).into_response(),
        Err(error) => failed(error),
    }
}

pub async fn remove<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::Own)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service.delete(id, None).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => failed(error),
    }
}

pub async fn remove_version<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, version)): Path<(Uuid, i64)>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::Edit)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service.delete(id, Some(version)).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => failed(error),
    }
}

fn stream(content: runinator_engine::services::WorkspaceContent) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/gzip")
        .header(header::CONTENT_LENGTH, content.size_bytes)
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=workspace.tar.gz",
        )
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(
            content.body,
        )))
        .unwrap_or_else(failed)
}

pub async fn download<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, version)): Path<(Uuid, i64)>,
    Query(query): Query<FileQuery>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    if let Some(path) = query.path {
        return match service.file(id, version, path).await {
            Ok(bytes) => (
                [
                    (header::CONTENT_TYPE, "application/octet-stream"),
                    (header::CONTENT_DISPOSITION, "attachment"),
                ],
                bytes,
            )
                .into_response(),
            Err(error) => failed(error),
        };
    }
    match service.open(id, version).await {
        Ok(content) => stream(content),
        Err(error) => failed(error),
    }
}

pub async fn restore<T: DatabaseImpl>(
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Query(query): Query<WorkerQuery>,
) -> Response {
    if let Err(reply) = ctx.require_system_role(&[SystemRole::Worker, SystemRole::Agent]) {
        return reply.into_reply().into_response();
    }
    let checkout = match service
        .require_assigned_checkout(id, query.replica_id, &ctx)
        .await
    {
        Ok(checkout) => checkout,
        Err(error) => return failed(error),
    };
    if checkout.base_version == 0 {
        return StatusCode::NO_CONTENT.into_response();
    }
    match service
        .open(checkout.workspace_id, checkout.base_version)
        .await
    {
        Ok(content) => stream(content),
        Err(error) => failed(error),
    }
}

pub async fn upload<T: DatabaseImpl>(
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Query(query): Query<WorkerQuery>,
    body: axum::body::Bytes,
) -> Response {
    if let Err(reply) = ctx.require_system_role(&[SystemRole::Worker, SystemRole::Agent]) {
        return reply.into_reply().into_response();
    }
    if let Err(error) = service
        .require_assigned_checkout(id, query.replica_id, &ctx)
        .await
    {
        return failed(error);
    }
    match service.upload(id, body.to_vec()).await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) => failed(error),
    }
}

pub fn routes<T: DatabaseImpl>(pool: Arc<T>) -> axum::Router {
    use axum::routing::{delete, get};
    axum::Router::new()
        .route("/workspaces", get(list::<T>).post(create::<T>))
        .route("/workspaces/{id}", get(detail::<T>).delete(remove::<T>))
        .route("/workspaces/{id}/versions", get(versions::<T>))
        .route(
            "/workspaces/{id}/versions/{version}",
            delete(remove_version::<T>),
        )
        .route(
            "/workspaces/{id}/versions/{version}/content",
            get(download::<T>),
        )
        .route(
            "/workspaces/checkouts/{id}/content",
            get(restore::<T>).post(upload::<T>),
        )
        .layer(Extension(pool))
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024))
}

pub const DOCS: &[EndpointDoc] = &[
    endpoint!(
        "get",
        "/workspaces",
        "Workspaces",
        "List workspaces",
        "List workspaces using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "post",
        "/workspaces",
        "Workspaces",
        "Create workspace",
        "Create workspace using durable workspace authorization.",
        false,
        None,
        &[],
        201,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/{id}",
        "Workspaces",
        "Inspect workspace",
        "Inspect workspace using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "delete",
        "/workspaces/{id}",
        "Workspaces",
        "Delete idle workspace",
        "Delete idle workspace using durable workspace authorization.",
        false,
        None,
        &[],
        204,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/{id}/versions",
        "Workspaces",
        "List workspace versions",
        "List workspace versions using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "delete",
        "/workspaces/{id}/versions/{version}",
        "Workspaces",
        "Delete historical version",
        "Delete historical version using durable workspace authorization.",
        false,
        None,
        &[],
        204,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/{id}/versions/{version}/content",
        "Workspaces",
        "Download workspace content",
        "Download workspace content using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/checkouts/{id}/content",
        "Workspaces",
        "Restore assigned checkout",
        "Restore assigned checkout using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "post",
        "/workspaces/checkouts/{id}/content",
        "Workspaces",
        "Upload assigned snapshot bytes",
        "Upload assigned snapshot bytes using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
];
