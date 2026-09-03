//! User-uploaded VM input files and reusable workspace file revisions.

use std::sync::Arc;

use axum::{
    Extension, Json,
    body::Body,
    extract::{Path, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_engine::services::WorkflowFiles;
use runinator_models::{
    auth::{AuthContext, Permission, ResourceType},
    rbac::Action,
};
use runinator_store::{RuntimeStore, roles::FileStore};
use runinator_ws_core::{
    models::ApiResponse,
    openapi::docs::{
        EndpointDoc, EndpointPolicy, Example, ParamDoc, RequestDoc, endpoint_with_policy,
    },
    responses::{api_error, bad_request, not_found},
};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct UploadFileQuery {
    pub path: String,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RuntimeFileQuery {
    pub consumer_run_id: Option<Uuid>,
}

fn mime_for(path: &str, supplied: Option<String>) -> String {
    supplied
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            mime_guess::from_path(path)
                .first_or_octet_stream()
                .essence_str()
                .to_string()
        })
}

/// Upload a reusable file. The current revision at the same virtual path is replaced atomically
/// in metadata, while the previous bytes remain available to already-started VM runs.
pub async fn upload_library_file<T: AuthorizationStore + FileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkflowFiles<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<UploadFileQuery>,
    body: axum::body::Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::Edit, ctx.selected_scope()) {
        return reply;
    }
    if ctx.org_id.is_none() {
        return bad_request("library files must be created inside an organization");
    }
    match service
        .publish_library(
            ctx.org_id,
            ctx.principal_id,
            query.path.clone(),
            mime_for(&query.path, query.mime_type),
            body.to_vec(),
        )
        .await
    {
        Ok(file) => {
            if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                .grant_resource_owner(ResourceType::LibraryFile, file.descriptor.id)
                .await
            {
                return reply;
            }
            (StatusCode::CREATED, Json(ApiResponse::WorkflowFile(file)))
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn list_library_files<T: AuthorizationStore + FileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkflowFiles<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::View, ctx.selected_scope()) {
        return reply;
    }
    match service.list_library(ctx.org_id).await {
        Ok(mut files) => {
            let visible = match AuthzChecker::new(db.as_ref(), &ctx)
                .visible_resource_ids(ResourceType::LibraryFile)
                .await
            {
                Ok(ids) => ids,
                Err(reply) => return reply,
            };
            if let Some(visible) = visible {
                files.retain(|file| visible.contains(&file.descriptor.id));
            }
            (StatusCode::OK, Json(ApiResponse::WorkflowFileList(files)))
        }
        Err(error) => api_error(error.to_string()),
    }
}

/// Upload bytes before a run starts. The caller owns the staged descriptor until run creation
/// claims it; this removes the start/upload race that existed with legacy artifacts.
pub async fn stage_workflow_file<T: AuthorizationStore + FileStore>(
    Extension(service): Extension<Arc<WorkflowFiles<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<UploadFileQuery>,
    body: axum::body::Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::Run, ctx.selected_scope()) {
        return reply;
    }
    match service
        .stage(
            ctx.org_id,
            ctx.principal_id,
            query.path.clone(),
            mime_for(&query.path, query.mime_type),
            body.to_vec(),
        )
        .await
    {
        Ok(file) => (StatusCode::CREATED, Json(ApiResponse::WorkflowFile(file))),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn archive_library_file<T: AuthorizationStore + FileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkflowFiles<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::Edit, ctx.selected_scope()) {
        return reply;
    }
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::LibraryFile, id, Permission::Edit)
        .await
    {
        return reply;
    }
    match service.archive(id, ctx.org_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(
                runinator_models::json!({ "success": true }),
            )),
        ),
        Ok(false) => not_found("workflow file not found"),
        Err(error) => api_error(error.to_string()),
    }
}

/// Stream a file only after checking the authenticated caller has workspace-level view access.
/// The object-store URI never reaches the browser, and workers use this same route with their
/// system credential when materializing an input for a provider.
pub async fn download_workflow_file<T: AuthorizationStore + FileStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<WorkflowFiles<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Query(query): Query<RuntimeFileQuery>,
) -> Response {
    let worker_or_agent = ctx
        .require_system_role(&[
            runinator_models::rbac::SystemRole::Worker,
            runinator_models::rbac::SystemRole::Agent,
        ])
        .is_ok();
    if !worker_or_agent
        && let Err(reply) = ctx.require_scope_action(Action::View, ctx.selected_scope())
    {
        return reply.into_response();
    }
    let file = match service.fetch(id).await {
        Ok(Some(file))
            if file.org_id == ctx.org_id && (!file.archived || file.workflow_run_id.is_some()) =>
        {
            file
        }
        Ok(_) => return (StatusCode::NOT_FOUND, "workflow file not found").into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };
    if worker_or_agent {
        let admitted = match query.consumer_run_id {
            Some(run_id) => service
                .run_admitted_file(run_id, id, file.workflow_run_id)
                .await
                .unwrap_or(false),
            None => false,
        };
        if !admitted {
            return (StatusCode::NOT_FOUND, "workflow file not found").into_response();
        }
    } else {
        match file.scope {
            runinator_models::files::FileScope::Staged => {
                if file.owner_id.is_none() || file.owner_id != ctx.principal_id {
                    return (StatusCode::NOT_FOUND, "workflow file not found").into_response();
                }
            }
            runinator_models::files::FileScope::Library => {
                if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                    .require_resource(ResourceType::LibraryFile, id, Permission::View)
                    .await
                {
                    return reply.into_response();
                }
            }
            runinator_models::files::FileScope::Run => {
                let Some(run_id) = file.workflow_run_id else {
                    return (StatusCode::NOT_FOUND, "workflow file not found").into_response();
                };
                let workflow_id = match service.workflow_id_for_run(run_id).await {
                    Ok(Some(workflow_id)) => workflow_id,
                    Ok(None) => {
                        return (StatusCode::NOT_FOUND, "workflow file not found").into_response();
                    }
                    Err(error) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                            .into_response();
                    }
                };
                if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                    .require_workflow(workflow_id, Permission::View)
                    .await
                {
                    return reply.into_response();
                }
            }
        }
    }
    let content = match service.open(&file).await {
        Ok(content) => content,
        Err(error) => return (StatusCode::NOT_FOUND, error.to_string()).into_response(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, file.descriptor.mime_type)
        .header(header::CONTENT_LENGTH, content.size_bytes)
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                file.descriptor.name.replace('"', "_")
            ),
        )
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(
            content.body,
        )))
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "response build failed").into_response()
        })
}

pub fn routes<T: AuthorizationStore + FileStore + RuntimeStore>() -> axum::Router {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_WORKFLOW_FILES,
            get(list_library_files::<T>).post(upload_library_file::<T>),
        )
        .route("/workflow_files/stage", post(stage_workflow_file::<T>))
        .route("/workflow_files/{id}", delete(archive_library_file::<T>))
        .route(
            "/workflow_files/{id}/content",
            get(download_workflow_file::<T>),
        )
}

const UPLOAD_QUERY: &[ParamDoc] = &[
    ParamDoc {
        name: "path",
        location: "query",
        description: "Safe relative path to preserve in the job workspace.",
        required: true,
        example: "fixtures/invoice.csv",
    },
    ParamDoc {
        name: "mime_type",
        location: "query",
        description: "Optional media type; inferred from the path when omitted.",
        required: false,
        example: "text/csv",
    },
];

const RAW_FILE_BODY: RequestDoc = RequestDoc {
    description: "Raw file bytes.",
    example: Example::Artifact,
    content_type: "application/octet-stream",
};

pub const DOCS: &[EndpointDoc] = &[
    endpoint_with_policy!(
        "get",
        "/workflow_files",
        "Workflow files",
        "List file library",
        "Lists the current workspace-library revisions available as VM file inputs.",
        EndpointPolicy::ScopedAction(Action::View),
        None,
        &[],
        200,
        "file library",
        Example::Workflow,
    ),
    endpoint_with_policy!(
        "post",
        "/workflow_files",
        "Workflow files",
        "Upload library file",
        "Stores a new reusable library revision. Existing runs keep the descriptor they already selected.",
        EndpointPolicy::ScopedAction(Action::Edit),
        Some(RAW_FILE_BODY),
        UPLOAD_QUERY,
        201,
        "stored file descriptor",
        Example::Artifact,
    ),
    endpoint_with_policy!(
        "post",
        "/workflow_files/stage",
        "Workflow files",
        "Stage run input file",
        "Stores raw bytes for one pending workflow run. The run request claims the resulting descriptor atomically from its owner.",
        EndpointPolicy::ScopedAction(Action::Run),
        Some(RAW_FILE_BODY),
        UPLOAD_QUERY,
        201,
        "staged file descriptor",
        Example::Artifact,
    ),
    endpoint_with_policy!(
        "delete",
        "/workflow_files/{id}",
        "Workflow files",
        "Archive library file",
        "Hides a reusable library revision without invalidating run-bound descriptors.",
        EndpointPolicy::ScopedAction(Action::Edit),
        None,
        &[],
        200,
        "archive result",
        Example::TaskResponse,
    ),
    endpoint_with_policy!(
        "get",
        "/workflow_files/{id}/content",
        "Workflow files",
        "Download file bytes",
        "Streams a file through the authenticated service; blob-store locations are never exposed to the client.",
        EndpointPolicy::Authenticated,
        None,
        &[],
        200,
        "file bytes",
        Example::Artifact,
    ),
];
