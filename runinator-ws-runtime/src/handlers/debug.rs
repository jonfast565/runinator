use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_comm::DebugVerb;
use runinator_models::auth::{AuthContext, Permission};
use runinator_store::{RuntimeStore, roles::WorkflowVmStore};
use serde::Deserialize;
use uuid::Uuid;

use runinator_engine::services::DebugOperations;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::bad_request;
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};

/// Unified VM debugger entrypoint. The VM supports continuation-scoped Step and Continue only;
/// reducer-era node mutation, breakpoint, and speculative-cursor commands were removed.
pub async fn debug_command<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(debug): Extension<Arc<DebugOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(verb): Json<DebugVerb>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    match debug.command(workflow_run_id, verb).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => bad_request(err.to_string()),
    }
}

/// Body of the continuation-scoped debugger verbs. Omitting it targets every operator-paused
/// continuation in the run.
#[derive(Deserialize, Default)]
pub struct CursorRequest {
    #[serde(default)]
    pub cursor: Option<Uuid>,
}

fn cursor_of(body: &Option<Json<CursorRequest>>) -> Option<Uuid> {
    body.as_ref().and_then(|Json(req)| req.cursor)
}

pub async fn step_debug_workflow_run<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(debug): Extension<Arc<DebugOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    body: Option<Json<CursorRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    match debug.step(workflow_run_id, cursor_of(&body)).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => bad_request(err.to_string()),
    }
}

pub async fn continue_debug_workflow_run<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(debug): Extension<Arc<DebugOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    body: Option<Json<CursorRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    match debug
        .continue_cursor(workflow_run_id, cursor_of(&body))
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => bad_request(err.to_string()),
    }
}

pub fn routes<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    pool: Arc<T>,
) -> axum::Router {
    use axum::routing::post;

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
            post(continue_debug_workflow_run::<T>).layer(Extension(pool)),
        )
}

pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/command",
        "Debug",
        "Run a VM debugger command",
        "Applies a continuation-scoped Step or Continue command to a workflow VM run.",
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
        "Step a VM continuation",
        "Advances an operator-paused continuation by one VM boundary.",
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
        "Continue a VM continuation",
        "Resumes one or all operator-paused workflow VM continuations.",
        false,
        None,
        &[],
        200,
        "workflow run continued",
        Example::TaskResponse,
    ),
];
