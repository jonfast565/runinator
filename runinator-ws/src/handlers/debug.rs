use std::sync::Arc;
use uuid::Uuid;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_comm::DebugVerb;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::{AuthContext, Permission};
use runinator_models::value::Value;
use serde::Deserialize;

use crate::events::{EventSender, emit_workflow_run};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::bad_request;

/// unified debug entrypoint: a single [`DebugVerb`] dispatched to the repository. the legacy
/// per-verb endpoints below remain as thin adapters for existing clients.
pub(crate) async fn debug_command<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(verb): Json<DebugVerb>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_run_workflow(db.as_ref(), &ctx, workflow_run_id, Permission::Run)
            .await
    {
        return reply;
    }
    match repository::apply_debug_command(db.as_ref(), workflow_run_id, verb).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

#[derive(Deserialize)]
pub(crate) struct RunToCursorRequest {
    pub(crate) node_id: String,
}

#[derive(Deserialize)]
pub(crate) struct SkipDebugRequest {
    pub(crate) output_json: Value,
    pub(crate) message: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct RerunNodeRequest {
    pub(crate) parameters: Value,
}

pub(crate) async fn step_debug_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_run_workflow(db.as_ref(), &ctx, workflow_run_id, Permission::Run)
            .await
    {
        return reply;
    }
    match repository::step_debug_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn continue_debug_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_run_workflow(db.as_ref(), &ctx, workflow_run_id, Permission::Run)
            .await
    {
        return reply;
    }
    match repository::continue_debug_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn update_workflow_run_debug<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(patch): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_run_workflow(db.as_ref(), &ctx, workflow_run_id, Permission::Run)
            .await
    {
        return reply;
    }
    match repository::update_workflow_run_debug(db.as_ref(), workflow_run_id, patch).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn run_to_cursor_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(req): Json<RunToCursorRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_run_workflow(db.as_ref(), &ctx, workflow_run_id, Permission::Run)
            .await
    {
        return reply;
    }
    match repository::run_to_cursor_workflow_run(db.as_ref(), workflow_run_id, req.node_id).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn skip_debug_workflow_node<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(req): Json<SkipDebugRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_run_workflow(db.as_ref(), &ctx, workflow_run_id, Permission::Run)
            .await
    {
        return reply;
    }
    match repository::skip_debug_workflow_node(
        db.as_ref(),
        workflow_run_id,
        req.output_json,
        req.message,
    )
    .await
    {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn rerun_debug_workflow_node<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(req): Json<RerunNodeRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_run_workflow(db.as_ref(), &ctx, workflow_run_id, Permission::Run)
            .await
    {
        return reply;
    }
    match repository::rerun_debug_workflow_node(db.as_ref(), workflow_run_id, req.parameters).await
    {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}
