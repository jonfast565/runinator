use std::sync::Arc;
use uuid::Uuid;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_models::{
    auth::{AuthContext, Permission},
    workflows::WorkflowTrigger,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, ScheduleStore},
};

use runinator_engine::services::SchedulingOperations;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{
    EndpointDoc, Example, WORKFLOW_TRIGGER_FILTERS, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::AuthContextExt;
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};

pub async fn upsert_workflow_trigger<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(scheduling): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
    Json(mut trigger): Json<WorkflowTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::Edit)
        .await
    {
        return reply;
    }
    trigger.workflow_id = workflow_id;
    match scheduling.save_workflow_trigger(&trigger, ctx.org_id).await {
        Ok(trigger) => (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_workflow_trigger<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(scheduling): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
    Json(mut trigger): Json<WorkflowTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_trigger_workflow(trigger_id, Permission::Edit)
        .await
    {
        return reply;
    }
    trigger.id = Some(trigger_id);
    match scheduling.save_workflow_trigger(&trigger, ctx.org_id).await {
        Ok(trigger) => (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_workflow_trigger<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(scheduling): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_trigger_workflow(trigger_id, Permission::View)
        .await
    {
        return reply;
    }
    match scheduling.fetch_workflow_trigger(trigger_id).await {
        Ok(Some(trigger)) => (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger))),
        Ok(None) => not_found(format!("Workflow trigger {trigger_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_workflow_triggers<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(scheduling): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::View)
        .await
    {
        return reply;
    }
    match scheduling.list_workflow_triggers(workflow_id).await {
        Ok(triggers) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowTriggerList(triggers)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_due_workflow_triggers<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(_db): Extension<Arc<T>>,
    Extension(scheduling): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Waker,
    ]) {
        return reply;
    }
    match scheduling.due_workflow_triggers().await {
        Ok(triggers) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowTriggerList(triggers)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_workflow_trigger<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(scheduling): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_trigger_workflow(trigger_id, Permission::Edit)
        .await
    {
        return reply;
    }
    match scheduling
        .delete_workflow_trigger(trigger_id, ctx.org_id)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

/// the `triggers` endpoints.
pub fn routes<T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::Extension;
    use axum::routing::get;
    axum::Router::new()
        .route(
            "/workflows/{id}/triggers",
            get(get_workflow_triggers::<T>)
                .post(upsert_workflow_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_WORKFLOW_TRIGGERS_DUE,
            get(get_due_workflow_triggers::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/{id}",
            get(get_workflow_trigger::<T>)
                .patch(update_workflow_trigger::<T>)
                .delete(delete_workflow_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/workflows/{id}/triggers",
        "Workflows",
        "List workflow triggers",
        "Lists triggers attached to one workflow definition.",
        false,
        None,
        &[],
        200,
        "workflow triggers",
        Example::TriggerList,
    ),
    endpoint(
        "post",
        "/workflows/{id}/triggers",
        "Workflows",
        "Create or replace a workflow trigger",
        "Creates or upserts a trigger for the workflow definition in the path.",
        false,
        json_body("Workflow trigger definition.", Example::Trigger),
        &[],
        200,
        "stored workflow trigger",
        Example::Trigger,
    ),
    endpoint(
        "get",
        "/workflow_triggers/due",
        "Control Plane",
        "List due workflow triggers",
        "Returns workflow triggers that are ready to fire. Used by scheduler loops and diagnostics.",
        false,
        None,
        WORKFLOW_TRIGGER_FILTERS,
        200,
        "due workflow triggers",
        Example::TriggerList,
    ),
    endpoint(
        "get",
        "/workflow_triggers/{id}",
        "Workflows",
        "Get a workflow trigger",
        "Fetches one workflow trigger by id.",
        false,
        None,
        &[],
        200,
        "workflow trigger",
        Example::Trigger,
    ),
    endpoint(
        "patch",
        "/workflow_triggers/{id}",
        "Workflows",
        "Update a workflow trigger",
        "Updates one workflow trigger by id.",
        false,
        json_body("Workflow trigger fields to store.", Example::Trigger),
        &[],
        200,
        "updated workflow trigger",
        Example::Trigger,
    ),
    endpoint(
        "delete",
        "/workflow_triggers/{id}",
        "Workflows",
        "Delete a workflow trigger",
        "Deletes one workflow trigger by id.",
        false,
        None,
        &[],
        200,
        "workflow trigger deleted",
        Example::TaskResponse,
    ),
];
