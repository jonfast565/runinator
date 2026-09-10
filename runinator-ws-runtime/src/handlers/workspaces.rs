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
    offset: Option<u64>,
    length: Option<usize>,
}
#[derive(Deserialize)]
pub struct WorkerQuery {
    replica_id: Uuid,
}
pub(super) fn failed(error: impl std::fmt::Display) -> Response {
    let message = error.to_string();
    let status = if message.contains("WORKSPACE007") {
        StatusCode::NOT_FOUND
    } else if message.contains("WORKSPACE013") {
        StatusCode::PAYLOAD_TOO_LARGE
    } else if message.contains("WORKSPACE003")
        || message.contains("WORKSPACE006")
        || message.contains("WORKSPACE005")
    {
        StatusCode::BAD_REQUEST
    } else if message.contains("WORKSPACE002") || message.contains("WORKSPACE009") {
        StatusCode::CONFLICT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, message).into_response()
}

pub async fn list<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(page): Query<Page>,
) -> Response {
    let checker = AuthzChecker::new(db.as_ref(), &ctx);
    let mut visible = Vec::new();
    let mut offset = 0;
    let mut skipped = 0;
    let limit = page.limit.clamp(1, 200) as usize;
    loop {
        let items = match service.list(ctx.org_id, 200, offset).await {
            Ok(items) => items,
            Err(error) => return failed(error),
        };
        let count = items.len();
        for workspace in items {
            match checker
                .resource_permission(ResourceType::Workspace, workspace.id)
                .await
            {
                Ok(Some(permission)) => {
                    if skipped < page.offset.max(0) {
                        skipped += 1;
                        continue;
                    }
                    visible.push(WorkspaceView {
                        workspace,
                        permission,
                    });
                    if visible.len() == limit {
                        return Json(visible).into_response();
                    }
                }
                Ok(None) => {}
                Err(reply) => return reply.into_response(),
            }
        }
        if count < 200 {
            break;
        }
        offset += 200;
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

pub async fn version_detail<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, version)): Path<(Uuid, i64)>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service.snapshot(id, version).await {
        Ok(snapshot) => Json(snapshot).into_response(),
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
        if let Some(length) = query.length {
            if length > 1024 * 1024 {
                return failed(
                    runinator_models::errors::WORKSPACE_INVALID
                        .error("preview length exceeds 1 MiB"),
                );
            }
            return match service
                .file_range(id, version, path, query.offset.unwrap_or(0), length)
                .await
            {
                Ok(bytes) => {
                    ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()
                }
                Err(error) => failed(error),
            };
        }
        return match service.file(id, version, path).await {
            Ok(content) => Response::builder()
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, content.size_bytes)
                .header(header::CONTENT_DISPOSITION, "attachment")
                .body(Body::from_stream(tokio_util::io::ReaderStream::new(
                    content.body,
                )))
                .unwrap_or_else(failed),
            Err(error) => failed(error),
        };
    }
    (
        StatusCode::CONFLICT,
        "Create an export transfer before downloading a workspace archive",
    )
        .into_response()
}

pub async fn upload_pack<T: DatabaseImpl>(
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
    match service.upload_pack(id, body.to_vec()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failed(error),
    }
}

#[derive(Deserialize)]
pub struct DirectoryQuery {
    #[serde(default)]
    path: String,
    cursor: Option<String>,
    limit: Option<usize>,
}

pub async fn directory<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, version)): Path<(Uuid, i64)>,
    Query(query): Query<DirectoryQuery>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service
        .directory(
            id,
            version,
            query.path,
            query.cursor,
            query.limit.unwrap_or(200),
        )
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(error) => failed(error),
    }
}

pub async fn object<T: DatabaseImpl>(
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, object)): Path<(Uuid, String)>,
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
    match service
        .object(checkout.workspace_id, object, checkout.base_version)
        .await
    {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response(),
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
    match service.checkout_content(checkout).await {
        Ok(content) => Response::builder()
            .header(
                header::CONTENT_TYPE,
                "application/vnd.runinator.workspace.native.v1+tar",
            )
            .header(header::CONTENT_LENGTH, content.size_bytes)
            .header(header::CONTENT_DISPOSITION, "attachment")
            .body(Body::from_stream(tokio_util::io::ReaderStream::new(
                content.body,
            )))
            .unwrap_or_else(failed),
        Err(error) => failed(error),
    }
}

pub async fn seal<T: DatabaseImpl>(
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Query(query): Query<WorkerQuery>,
    ValidatedJson(request): ValidatedJson<WorkspaceSeal>,
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
    match service.seal(id, request).await {
        Ok(receipt) => Json(receipt).into_response(),
        Err(error) => failed(error),
    }
}

#[derive(Deserialize)]
pub struct ResultQuery {
    name: String,
    #[serde(default)]
    preview: bool,
}

pub async fn results<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, version)): Path<(Uuid, i64)>,
    Query(query): Query<DirectoryQuery>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service
        .results(id, version, query.cursor, query.limit.unwrap_or(200))
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(error) => failed(error),
    }
}

pub async fn result_content<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, version)): Path<(Uuid, i64)>,
    Query(query): Query<ResultQuery>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service
        .result_content(id, version, query.name, query.preview)
        .await
    {
        Ok(content) => Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, content.size_bytes)
            .header(header::CONTENT_DISPOSITION, "attachment")
            .body(Body::from_stream(tokio_util::io::ReaderStream::new(
                content.body,
            )))
            .unwrap_or_else(failed),
        Err(error) => failed(error),
    }
}

pub async fn download_ticket<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, version)): Path<(Uuid, i64)>,
    ValidatedJson(request): ValidatedJson<WorkspaceDownloadRequest>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service.download_ticket(id, version, request).await {
        Ok(ticket) => Json(ticket).into_response(),
        Err(error) => failed(error),
    }
}
pub async fn ticket_content<T: DatabaseImpl>(
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Path(ticket): Path<Uuid>,
) -> Response {
    match service.ticket_content(ticket).await {
        Ok(content) => Response::builder()
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, content.size_bytes)
            .header(
                header::CONTENT_DISPOSITION,
                "attachment; filename=workspace-download",
            )
            .header(header::CACHE_CONTROL, "private, no-store")
            .body(Body::from_stream(tokio_util::io::ReaderStream::new(
                content.body,
            )))
            .unwrap_or_else(failed),
        Err(error) => failed(error),
    }
}

#[derive(Deserialize)]
pub struct DiffQuery {
    before: i64,
    cursor: Option<String>,
}
pub async fn diff<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkspaceService<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, version)): Path<(Uuid, i64)>,
    Query(query): Query<DiffQuery>,
) -> Response {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Workspace, id, Permission::View)
        .await
    {
        return reply.into_reply().into_response();
    }
    match service.diff(id, query.before, version, query.cursor).await {
        Ok(page) => Json(page).into_response(),
        Err(error) => failed(error),
    }
}

pub fn routes<T: DatabaseImpl>(pool: Arc<T>) -> axum::Router {
    use axum::routing::get;
    axum::Router::new()
        .merge(super::workspace_transfers::routes::<T>())
        .route("/workspaces", get(list::<T>).post(create::<T>))
        .route("/workspaces/{id}", get(detail::<T>).delete(remove::<T>))
        .route("/workspaces/{id}/versions", get(versions::<T>))
        .route(
            "/workspaces/{id}/versions/{version}",
            get(version_detail::<T>).delete(remove_version::<T>),
        )
        .route(
            "/workspaces/{id}/versions/{version}/content",
            get(download::<T>),
        )
        .route(
            "/workspaces/{id}/versions/{version}/entries",
            get(directory::<T>),
        )
        .route(
            "/workspaces/checkouts/{id}/objects/{object}",
            get(object::<T>),
        )
        .route("/workspaces/checkouts/{id}/content", get(restore::<T>))
        .route(
            "/workspaces/checkouts/{id}/packs",
            axum::routing::post(upload_pack::<T>),
        )
        .route(
            "/workspaces/checkouts/{id}/seal",
            axum::routing::post(seal::<T>),
        )
        .route(
            "/workspaces/{id}/versions/{version}/results",
            get(results::<T>),
        )
        .route(
            "/workspaces/{id}/versions/{version}/result",
            get(result_content::<T>),
        )
        .route(
            "/workspaces/{id}/versions/{version}/downloads",
            axum::routing::post(download_ticket::<T>),
        )
        .route("/workspace-downloads/{ticket}", get(ticket_content::<T>))
        .route("/workspaces/{id}/versions/{version}/diff", get(diff::<T>))
        .layer(Extension(pool))
        .layer(DefaultBodyLimit::max(80 * 1024 * 1024))
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
        "post",
        "/workspaces/checkouts/{id}/packs",
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
    endpoint!(
        "get",
        "/workspaces/{id}/versions/{version}/entries",
        "Workspaces",
        "Page workspace directory",
        "Page workspace directory using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/checkouts/{id}/objects/{object}",
        "Workspaces",
        "Read assigned workspace object",
        "Read assigned workspace object using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "post",
        "/workspaces/checkouts/{id}/seal",
        "Workspaces",
        "Validate workspace revision",
        "Validate workspace revision using durable workspace authorization.",
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
        "Restore one assigned checkout as a single native workspace archive.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/{id}/versions/{version}/results",
        "Workspaces",
        "Read workspace results",
        "Read workspace results using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/{id}/versions/{version}/result",
        "Workspaces",
        "Read workspace result",
        "Read workspace result using durable workspace authorization.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "post",
        "/workspaces/{id}/versions/{version}/downloads",
        "Workspaces",
        "Create scoped download",
        "Create an expiring download scoped to one immutable workspace resource.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspace-downloads/{ticket}",
        "Workspaces",
        "Download ticket content",
        "Stream the exact resource authorized by an expiring download ticket.",
        true,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/{id}/versions/{version}/diff",
        "Workspaces",
        "Compare workspace revisions",
        "Page filesystem and result changes between two immutable versions.",
        false,
        None,
        &[],
        200,
        "workspace response",
        Example::TaskResponse
    ),
    endpoint!(
        "post",
        "/workspaces/{id}/transfers",
        "Workspaces",
        "Create workspace transfer",
        "Create workspace transfer with workspace authorization and a fenced durable job.",
        false,
        None,
        &[],
        202,
        "workspace transfer response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspace-transfers/{id}",
        "Workspaces",
        "Read workspace transfer",
        "Read workspace transfer with workspace authorization and a fenced durable job.",
        false,
        None,
        &[],
        200,
        "workspace transfer response",
        Example::TaskResponse
    ),
    endpoint!(
        "delete",
        "/workspace-transfers/{id}",
        "Workspaces",
        "Cancel workspace transfer",
        "Cancel workspace transfer with workspace authorization and a fenced durable job.",
        false,
        None,
        &[],
        204,
        "workspace transfer response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspace-transfers/{id}/content",
        "Workspaces",
        "Download completed workspace export",
        "Download completed workspace export with workspace authorization and a fenced durable job.",
        false,
        None,
        &[],
        200,
        "workspace transfer response",
        Example::TaskResponse
    ),
    endpoint!(
        "put",
        "/workspace-transfers/{id}/content",
        "Workspaces",
        "Upload workspace import archive",
        "Upload workspace import archive with workspace authorization and a fenced durable job.",
        false,
        None,
        &[],
        202,
        "workspace transfer response",
        Example::TaskResponse
    ),
    endpoint!(
        "get",
        "/workspaces/{id}/versions/{version}",
        "Workspaces",
        "Read immutable version summary",
        "Fetch a pinned version independently of history pagination.",
        false,
        None,
        &[],
        200,
        "workspace version",
        Example::TaskResponse
    ),
];
