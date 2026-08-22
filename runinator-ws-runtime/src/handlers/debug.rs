use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_comm::DebugVerb;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::{AuthContext, Permission};
use serde::Deserialize;
use uuid::Uuid;

use runinator_engine::repository;
use runinator_ws_core::events::{EventSender, emit_workflow_run};
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::bad_request;
use runinator_ws_middleware::authz::AuthzChecker;

/// Unified VM debugger entrypoint. The VM supports continuation-scoped Step and Continue only;
/// reducer-era node mutation, breakpoint, and speculative-cursor commands were removed.
pub async fn debug_command<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
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
    match repository::apply_debug_command(db.as_ref(), workflow_run_id, verb).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
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

pub async fn step_debug_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
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
    match repository::step_debug_cursor(db.as_ref(), workflow_run_id, cursor_of(&body)).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub async fn continue_debug_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
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
    match repository::continue_debug_cursor(db.as_ref(), workflow_run_id, cursor_of(&body)).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub fn routes<T: DatabaseImpl>(pool: Arc<T>) -> axum::Router {
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
