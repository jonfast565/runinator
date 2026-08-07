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
use crate::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
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

/// the `debug` endpoints.
pub(crate) fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{patch, post};
    axum::Router::new()
        .route(
            "/workflow_runs/{id}/debug/command",
            post(debug_command::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/step",
            post(step_debug_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/continue",
            post(continue_debug_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug",
            patch(update_workflow_run_debug::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/run_to_cursor",
            post(run_to_cursor_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/skip",
            post(skip_debug_workflow_node::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/rerun_node",
            post(rerun_debug_workflow_node::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub(crate) const DOCS: &[EndpointDoc] = &[
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/command",
        "Debug",
        "Run a debugger command",
        "Applies a debugger command to a paused or debuggable workflow run.",
        false,
        json_body("Debugger command payload.", Example::AutomationRecord),
        &[],
        200,
        "debug command applied",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/step",
        "Debug",
        "Step a workflow run",
        "Advances a debug-paused workflow run by one reducer step.",
        false,
        None,
        &[],
        200,
        "debug step applied",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/continue",
        "Debug",
        "Continue a workflow run",
        "Continues a debug-paused workflow run.",
        false,
        None,
        &[],
        200,
        "workflow run continued",
        Example::TaskResponse,
    ),
    endpoint(
        "patch",
        "/workflow_runs/{id}/debug",
        "Debug",
        "Update debugger state",
        "Updates debugger flags or breakpoints for a workflow run.",
        false,
        json_body("Debug state patch.", Example::AutomationRecord),
        &[],
        200,
        "debug state updated",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/run_to_cursor",
        "Debug",
        "Run to debugger cursor",
        "Continues a debug-paused run until the requested node or breakpoint is reached.",
        false,
        json_body("Debugger cursor target.", Example::AutomationRecord),
        &[],
        200,
        "run-to-cursor started",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/skip",
        "Debug",
        "Skip a debug node",
        "Skips the selected node while debugging a workflow run.",
        false,
        json_body("Node skip request.", Example::AutomationRecord),
        &[],
        200,
        "debug node skipped",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/rerun_node",
        "Debug",
        "Rerun a debug node",
        "Reruns a selected node while debugging a workflow run.",
        false,
        json_body("Node rerun request.", Example::AutomationRecord),
        &[],
        200,
        "debug node rerun requested",
        Example::TaskResponse,
    ),
];
