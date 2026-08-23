use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_models::{
    api_routes::{WORKFLOW_JSON_IMPORT_RISK_ACK, WORKFLOW_JSON_IMPORT_RISK_HEADER},
    auth::{AuthContext, Permission},
    errors::error_code_or_unknown,
    value::Value,
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowDuplicateRequest, WorkflowSimulateRequest,
    },
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, FunctionStore, NotificationStore, ScheduleStore, WorkflowVmStore},
};
use serde::Deserialize;

use runinator_engine::services::WorkflowAuthoring;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{
    EndpointDoc, Example, WORKFLOW_FILTERS, WORKFLOW_IMPORT_HEADERS, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, bad_request, not_found, validation_error};
use runinator_ws_middleware::authz::AuthContextExt;
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};

pub async fn upsert_workflow<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut workflow): Json<WorkflowDefinition>,
) -> (StatusCode, Json<ApiResponse>) {
    // updating an existing workflow requires edit; creating one stamps the creator as owner.
    let is_update = workflow.id.is_some();
    if let Some(id) = workflow.id {
        if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_workflow(id, Permission::Edit)
            .await
        {
            return reply;
        }
        // preserve the stored org on update so a client cannot re-tenant a workflow by editing it.
        workflow.org_id = match authoring.fetch(id).await {
            Ok(Some(existing)) => existing.org_id,
            Ok(None) => workflow.org_id,
            Err(err) => return api_error(err.to_string()),
        };
    } else {
        // a new workflow is owned by the creator's active org (None = platform-global).
        workflow.org_id = ctx.org_id;
    }
    match authoring.save(&workflow, &ctx.revision_author()).await {
        Ok(workflow) => {
            if !is_update && let Some(id) = workflow.id {
                if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx).grant_owner(id).await {
                    return reply;
                }
            }
            (StatusCode::OK, Json(ApiResponse::Workflow(workflow)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn validate_workflow<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Json(workflow): Json<WorkflowDefinition>,
) -> (StatusCode, Json<ApiResponse>) {
    match authoring.validate(&workflow).await {
        Ok(workflow) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
        Err(err) => validation_error(err.as_ref()),
    }
}

/// dry-run a workflow with the VM's evaluators against live config, publishing no actions.
/// A saved workflow requires `Run`; an unsaved draft only needs an authenticated caller. When
/// `replay_run` is set, that run's recorded outputs drive the walk, so it is gated on the run too.
pub async fn simulate_workflow<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<WorkflowSimulateRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(id) = request.workflow.id
        && let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_workflow(id, Permission::Run)
            .await
    {
        return reply;
    }
    if let Some(run_id) = request.replay_run {
        match authoring.workflow_run(run_id).await {
            Ok(Some(run)) => {
                if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                    .require_workflow(run.workflow_id, Permission::Run)
                    .await
                {
                    return reply;
                }
            }
            Ok(None) => return not_found(format!("Workflow run {run_id} not found")),
            Err(err) => return api_error(err.to_string()),
        }
    }
    match authoring
        .simulate(&request.workflow, request.inputs, request.replay_run)
        .await
    {
        Ok(run) => match Value::encode(&run) {
            Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value))),
            Err(err) => api_error(err.to_string()),
        },
        Err(err) => bad_request(err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct WorkflowQuery {
    pub name: Option<String>,
}

/// list workflow definitions visible to the caller.
#[utoipa::path(
    get,
    path = "/workflows",
    tag = "Workflows",
    responses((status = 200, description = "workflow definitions", body = serde_json::Value)),
)]
pub async fn get_workflows<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<WorkflowQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(name) = query.name {
        return match authoring.fetch_by_name(name).await {
            Ok(Some(workflow)) => match workflow.id {
                Some(id)
                    if AuthzChecker::new(db.as_ref(), &ctx)
                        .require_workflow(id, Permission::View)
                        .await
                        .is_err() =>
                {
                    not_found("Workflow not found")
                }
                _ => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
            },
            Ok(None) => not_found("Workflow not found"),
            Err(err) => api_error(err.to_string()),
        };
    }

    match authoring.list().await {
        Ok(workflows) => {
            let visible = AuthzChecker::new(db.as_ref(), &ctx)
                .visible_workflow_ids()
                .await;
            let workflows = match visible {
                Ok(Some(ids)) => workflows
                    .into_iter()
                    .filter(|workflow| workflow.id.is_some_and(|id| ids.contains(&id)))
                    .collect(),
                Ok(None) => workflows,
                Err(reply) => return reply,
            };
            (StatusCode::OK, Json(ApiResponse::WorkflowList(workflows)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/workflows/import",
    tag = "Packs",
    params(
        (
            "x-runinator-json-workflow-risk",
            Header,
            description = "Required to acknowledge the risk of importing a raw JSON workflow bundle.",
            example = "system-breakage-possible"
        )
    ),
    request_body(
        description = "A raw workflow bundle JSON payload. This path is the legacy non-zip import flow.",
        content(("application/json"))
    ),
    responses(
        (status = 200, description = "workflow bundle imported", body = serde_json::Value),
        (status = 400, description = "invalid bundle or missing risk acknowledgment", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn import_workflow_bundle<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    Json(bundle): Json<WorkflowBundle>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::Edit, ctx.selected_scope())
    {
        return reply;
    }
    if !json_workflow_import_risk_acknowledged(&headers) {
        return json_workflow_import_risk_required();
    }
    import_acknowledged_workflow_bundle(authoring, ctx.org_id, bundle).await
}

pub async fn import_acknowledged_workflow_bundle<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    authoring: Arc<WorkflowAuthoring<T>>,
    org_id: Option<Uuid>,
    bundle: WorkflowBundle,
) -> (StatusCode, Json<ApiResponse>) {
    log::info!(
        "Importing workflow bundle: {} workflows, {} triggers",
        bundle.workflows.len(),
        bundle.triggers.len()
    );
    match authoring.import(bundle, false, org_id).await {
        Ok(bundle) => {
            log::info!("Imported workflow bundle successfully");
            (StatusCode::OK, Json(ApiResponse::WorkflowBundle(bundle)))
        }
        Err(err) => {
            log::error!(
                "Failed to import workflow bundle ({}): {}",
                error_code_or_unknown(err.as_ref()),
                err
            );
            api_error(err.to_string())
        }
    }
}

pub fn json_workflow_import_risk_acknowledged(headers: &HeaderMap) -> bool {
    headers
        .get(WORKFLOW_JSON_IMPORT_RISK_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case(WORKFLOW_JSON_IMPORT_RISK_ACK))
}

pub fn json_workflow_import_risk_required() -> (StatusCode, Json<ApiResponse>) {
    bad_request(format!(
        "raw JSON workflow imports can break system behavior; set header {WORKFLOW_JSON_IMPORT_RISK_HEADER}: {WORKFLOW_JSON_IMPORT_RISK_ACK} to acknowledge the risk"
    ))
}

pub async fn export_workflow_bundle<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    match authoring.export(None).await {
        Ok(mut bundle) => {
            if let Some(ids) = match AuthzChecker::new(db.as_ref(), &ctx)
                .visible_workflow_ids()
                .await
            {
                Ok(ids) => ids,
                Err(reply) => return reply,
            } {
                bundle
                    .workflows
                    .retain(|workflow| workflow.id.is_some_and(|id| ids.contains(&id)));
                bundle
                    .triggers
                    .retain(|trigger| ids.contains(&trigger.workflow_id));
            }
            (StatusCode::OK, Json(ApiResponse::WorkflowBundle(bundle)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn export_single_workflow_bundle<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::View)
        .await
    {
        return reply;
    }
    match authoring.export(Some(workflow_id)).await {
        Ok(bundle) if bundle.workflows.is_empty() => {
            not_found(format!("Workflow {workflow_id} not found"))
        }
        Ok(bundle) => (StatusCode::OK, Json(ApiResponse::WorkflowBundle(bundle))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_workflow<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::View)
        .await
    {
        return reply;
    }
    match authoring.fetch(workflow_id).await {
        // a cross-tenant workflow is not-found even if a stray grant would otherwise reveal it.
        Ok(Some(workflow)) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
        Ok(None) => not_found(format!("Workflow {workflow_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// how many revisions a list returns by default, and the ceiling a caller can ask for. history is
/// small per row but unbounded over time, so the endpoint pages rather than returning everything.
const DEFAULT_REVISION_LIMIT: i64 = 50;
const MAX_REVISION_LIMIT: i64 = 500;

#[derive(Debug, Deserialize)]
pub struct RevisionListQuery {
    limit: Option<i64>,
}

/// list a workflow's revision history, newest first.
#[utoipa::path(
    get,
    path = "/workflows/{id}/revisions",
    tag = "Workflows",
    params(
        ("id" = Uuid, Path, description = "workflow id"),
        ("limit" = Option<i64>, Query, description = "maximum revisions to return (default 50, max 500)"),
    ),
    responses((status = 200, description = "revision history", body = serde_json::Value)),
)]
pub async fn get_workflow_revisions<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
    Query(query): Query<RevisionListQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::View)
        .await
    {
        return reply;
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_REVISION_LIMIT)
        .clamp(1, MAX_REVISION_LIMIT);
    match authoring.revisions(workflow_id, limit).await {
        Ok(revisions) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowRevisionList(revisions)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// fetch one revision, including the full definition it captured.
#[utoipa::path(
    get,
    path = "/workflows/{id}/revisions/{revision}",
    tag = "Workflows",
    params(
        ("id" = Uuid, Path, description = "workflow id"),
        ("revision" = i64, Path, description = "per-workflow revision number"),
    ),
    responses(
        (status = 200, description = "the revision", body = serde_json::Value),
        (status = 404, description = "no such revision"),
    ),
)]
pub async fn get_workflow_revision<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((workflow_id, revision)): Path<(Uuid, i64)>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::View)
        .await
    {
        return reply;
    }
    match authoring.revision(workflow_id, revision).await {
        Ok(Some(found)) => (StatusCode::OK, Json(ApiResponse::WorkflowRevision(found))),
        Ok(None) => not_found(format!("Workflow {workflow_id} has no revision {revision}")),
        Err(err) => api_error(err.to_string()),
    }
}

/// restore an earlier revision as the workflow's current definition.
///
/// gated on `Edit` rather than a platform capability: rollback is a write to one workflow, so the
/// resource grant that governs editing it governs this too.
#[utoipa::path(
    post,
    path = "/workflows/{id}/revisions/{revision}/restore",
    tag = "Workflows",
    params(
        ("id" = Uuid, Path, description = "workflow id"),
        ("revision" = i64, Path, description = "revision to restore"),
    ),
    responses(
        (status = 200, description = "the restored definition, saved as a new revision", body = serde_json::Value),
        (status = 404, description = "no such workflow or revision"),
    ),
)]
pub async fn restore_workflow_revision<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((workflow_id, revision)): Path<(Uuid, i64)>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::Edit)
        .await
    {
        return reply;
    }
    match authoring
        .restore_revision(workflow_id, revision, &ctx.revision_author())
        .await
    {
        Ok(workflow) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn duplicate_workflow<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
    Query(request): Query<WorkflowDuplicateRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::View)
        .await
    {
        return reply;
    }
    match authoring
        .duplicate(
            workflow_id,
            request.bump,
            &ctx.revision_author(),
            ctx.org_id,
        )
        .await
    {
        Ok(workflow) => {
            if let Some(id) = workflow.id {
                if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx).grant_owner(id).await {
                    return reply;
                }
            }
            (StatusCode::OK, Json(ApiResponse::Workflow(workflow)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_workflow<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(authoring): Extension<Arc<WorkflowAuthoring<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::Edit)
        .await
    {
        return reply;
    }
    match authoring.delete(workflow_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

/// the `workflows` endpoints.
pub fn routes<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_WORKFLOWS,
            get(get_workflows::<T>)
                .post(upsert_workflow::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_WORKFLOWS_VALIDATE,
            post(validate_workflow::<T>).layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_WORKFLOWS_SIMULATE,
            post(simulate_workflow::<T>).layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_WORKFLOWS_IMPORT,
            post(import_workflow_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_WORKFLOWS_EXPORT,
            get(export_workflow_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}",
            get(get_workflow::<T>)
                .patch(upsert_workflow::<T>)
                .delete(delete_workflow::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/export",
            get(export_single_workflow_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/duplicate",
            post(duplicate_workflow::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/revisions",
            get(get_workflow_revisions::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/revisions/{revision}",
            get(get_workflow_revision::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/revisions/{revision}/restore",
            post(restore_workflow_revision::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/workflows",
        "Workflows",
        "List workflow definitions",
        "Lists workflow definitions visible to the caller. Supplying `name` returns the matching workflow instead of the full list.",
        false,
        None,
        WORKFLOW_FILTERS,
        200,
        "workflow definitions",
        Example::WorkflowList,
    ),
    endpoint(
        "post",
        "/workflows",
        "Workflows",
        "Create or replace a workflow",
        "Stores a workflow definition. New workflows are owned by the creator; updating an existing workflow requires edit access.",
        false,
        json_body(
            "Workflow definition to create or replace.",
            Example::Workflow,
        ),
        &[],
        200,
        "stored workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "post",
        "/workflows/validate",
        "Workflows",
        "Validate a workflow definition",
        "Validates a workflow against graph, typing, provider, and config rules without saving it.",
        false,
        json_body("Workflow definition to validate.", Example::Workflow),
        &[],
        200,
        "validated workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "post",
        "/workflows/import",
        "Packs",
        "Import a raw workflow bundle",
        "Legacy JSON bundle import. This is intentionally guarded because raw JSON can bypass REXRAP well-formedness constraints.",
        false,
        json_body("Raw workflow bundle JSON.", Example::WorkflowBundle),
        WORKFLOW_IMPORT_HEADERS,
        200,
        "imported workflow bundle",
        Example::WorkflowBundle,
    ),
    endpoint(
        "get",
        "/workflows/export",
        "Packs",
        "Export visible workflows",
        "Exports the caller's visible workflow definitions and triggers as a JSON workflow bundle.",
        false,
        None,
        &[],
        200,
        "workflow bundle",
        Example::WorkflowBundle,
    ),
    endpoint(
        "get",
        "/workflows/{id}",
        "Workflows",
        "Get a workflow",
        "Fetches one workflow definition by id if the caller has view access.",
        false,
        None,
        &[],
        200,
        "workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "patch",
        "/workflows/{id}",
        "Workflows",
        "Update a workflow",
        "Replaces the stored workflow definition for the id in the path. The request body should carry the full workflow definition.",
        false,
        json_body("Workflow definition to store.", Example::Workflow),
        &[],
        200,
        "updated workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "delete",
        "/workflows/{id}",
        "Workflows",
        "Delete a workflow",
        "Deletes a workflow definition. The caller must have edit access.",
        false,
        None,
        &[],
        200,
        "workflow deleted",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/workflows/{id}/export",
        "Packs",
        "Export one workflow",
        "Exports one workflow definition and its triggers as a JSON workflow bundle.",
        false,
        None,
        &[],
        200,
        "workflow bundle",
        Example::WorkflowBundle,
    ),
    endpoint(
        "post",
        "/workflows/{id}/duplicate",
        "Workflows",
        "Duplicate a workflow",
        "Creates a copy of a workflow. The optional `bump` query in the model controls version bump behavior.",
        false,
        None,
        &[],
        200,
        "duplicated workflow",
        Example::Workflow,
    ),
];
