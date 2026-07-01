use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    api_routes::{WORKFLOW_JSON_IMPORT_RISK_ACK, WORKFLOW_JSON_IMPORT_RISK_HEADER},
    auth::{AuthContext, Permission},
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowDuplicateRequest},
};
use serde::Deserialize;

use crate::authz;
use crate::events::{AppEvent, EventSender, emit};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, bad_request, not_found, validation_error};

pub(crate) async fn upsert_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut workflow): Json<WorkflowDefinition>,
) -> (StatusCode, Json<ApiResponse>) {
    // updating an existing workflow requires edit; creating one stamps the creator as owner.
    let is_update = workflow.id.is_some();
    if let Some(id) = workflow.id {
        if let Err(reply) = authz::require_workflow(db.as_ref(), &ctx, id, Permission::Edit).await {
            return reply;
        }
        // preserve the stored org on update so a client cannot re-tenant a workflow by editing it.
        workflow.org_id = match repository::fetch_workflow(db.as_ref(), id).await {
            Ok(Some(existing)) => existing.org_id,
            Ok(None) => workflow.org_id,
            Err(err) => return api_error(err.to_string()),
        };
    } else {
        // a new workflow is owned by the creator's active org (None = platform-global).
        workflow.org_id = ctx.org_id;
    }
    match repository::upsert_workflow(db.as_ref(), &workflow).await {
        Ok(workflow) => {
            if !is_update {
                if let Some(id) = workflow.id {
                    authz::grant_owner(db.as_ref(), &ctx, id).await;
                }
            }
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::Workflow(workflow)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn validate_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(workflow): Json<WorkflowDefinition>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::validate_workflow_definition_with_catalog(db.as_ref(), &workflow).await {
        Ok(workflow) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
        Err(err) => validation_error(err.as_ref()),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkflowQuery {
    pub(crate) name: Option<String>,
}

/// list workflow definitions visible to the caller.
#[utoipa::path(
    get,
    path = "/workflows",
    tag = "Workflows",
    responses((status = 200, description = "workflow definitions", body = serde_json::Value)),
)]
pub(crate) async fn get_workflows<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<WorkflowQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(name) = query.name {
        return match repository::fetch_workflow_by_name(db.as_ref(), name).await {
            // hide cross-tenant workflows behind a not-found before consulting grants.
            Ok(Some(workflow)) if !authz::org_visible(&ctx, workflow.org_id) => {
                not_found("Workflow not found")
            }
            Ok(Some(workflow)) => match workflow.id {
                Some(id)
                    if authz::require_workflow(db.as_ref(), &ctx, id, Permission::View)
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

    match repository::fetch_workflows(db.as_ref()).await {
        Ok(workflows) => {
            // scope to the caller's org first (cross-tenant workflows are never listed), then to the
            // grant-based visibility set (None = admin/auth-disabled = all grant-visible).
            let workflows: Vec<_> = workflows
                .into_iter()
                .filter(|workflow| authz::org_visible(&ctx, workflow.org_id))
                .collect();
            let visible = authz::visible_workflow_ids(db.as_ref(), &ctx).await;
            let workflows = match visible {
                Some(ids) => workflows
                    .into_iter()
                    .filter(|workflow| workflow.id.is_some_and(|id| ids.contains(&id)))
                    .collect(),
                None => workflows,
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
        (status = 400, description = "invalid bundle or missing risk acknowledgment", body = crate::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = crate::models::ApiError),
    ),
)]
pub(crate) async fn import_workflow_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    Json(bundle): Json<WorkflowBundle>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authz::require_admin(&ctx) {
        return reply;
    }
    if !json_workflow_import_risk_acknowledged(&headers) {
        return json_workflow_import_risk_required();
    }
    import_acknowledged_workflow_bundle(db, events, bundle).await
}

pub(crate) async fn import_acknowledged_workflow_bundle<T: DatabaseImpl>(
    db: Arc<T>,
    events: EventSender,
    bundle: WorkflowBundle,
) -> (StatusCode, Json<ApiResponse>) {
    log::info!(
        "Importing workflow bundle: {} workflows, {} triggers",
        bundle.workflows.len(),
        bundle.triggers.len()
    );
    match repository::import_workflow_bundle(db.as_ref(), bundle).await {
        Ok(bundle) => {
            log::info!("Imported workflow bundle successfully");
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::WorkflowBundle(bundle)))
        }
        Err(err) => {
            log::error!("Failed to import workflow bundle: {}", err);
            api_error(err.to_string())
        }
    }
}

pub(crate) fn json_workflow_import_risk_acknowledged(headers: &HeaderMap) -> bool {
    headers
        .get(WORKFLOW_JSON_IMPORT_RISK_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case(WORKFLOW_JSON_IMPORT_RISK_ACK))
}

pub(crate) fn json_workflow_import_risk_required() -> (StatusCode, Json<ApiResponse>) {
    bad_request(format!(
        "raw JSON workflow imports can break system behavior; set header {WORKFLOW_JSON_IMPORT_RISK_HEADER}: {WORKFLOW_JSON_IMPORT_RISK_ACK} to acknowledge the risk"
    ))
}

pub(crate) async fn export_workflow_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::export_workflow_bundle(db.as_ref(), None).await {
        Ok(mut bundle) => {
            if let Some(ids) = authz::visible_workflow_ids(db.as_ref(), &ctx).await {
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

pub(crate) async fn export_single_workflow_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_workflow(db.as_ref(), &ctx, workflow_id, Permission::View).await
    {
        return reply;
    }
    match repository::export_workflow_bundle(db.as_ref(), Some(workflow_id)).await {
        Ok(bundle) if bundle.workflows.is_empty() => {
            not_found(format!("Workflow {workflow_id} not found"))
        }
        Ok(bundle) => (StatusCode::OK, Json(ApiResponse::WorkflowBundle(bundle))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_workflow(db.as_ref(), &ctx, workflow_id, Permission::View).await
    {
        return reply;
    }
    match repository::fetch_workflow(db.as_ref(), workflow_id).await {
        // a cross-tenant workflow is not-found even if a stray grant would otherwise reveal it.
        Ok(Some(workflow)) if !authz::org_visible(&ctx, workflow.org_id) => {
            not_found(format!("Workflow {workflow_id} not found"))
        }
        Ok(Some(workflow)) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
        Ok(None) => not_found(format!("Workflow {workflow_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn duplicate_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
    Query(request): Query<WorkflowDuplicateRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_workflow(db.as_ref(), &ctx, workflow_id, Permission::View).await
    {
        return reply;
    }
    match repository::duplicate_workflow(db.as_ref(), workflow_id, request.bump).await {
        Ok(workflow) => {
            if let Some(id) = workflow.id {
                authz::grant_owner(db.as_ref(), &ctx, id).await;
            }
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::Workflow(workflow)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_workflow(db.as_ref(), &ctx, workflow_id, Permission::Edit).await
    {
        return reply;
    }
    match repository::delete_workflow(db.as_ref(), workflow_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}
