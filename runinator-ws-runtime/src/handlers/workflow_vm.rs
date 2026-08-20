//! Operator reads for the compiled workflow VM.
//!
//! These intentionally expose durable continuations, effects, and journal records directly.
//! A VM-backed run must never reconstruct its history from legacy node-run rows.

use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::{AuthContext, Permission},
    workflow_vm::WorkflowVmCursor,
};
use uuid::Uuid;

use runinator_ws_core::{
    models::ApiResponse,
    openapi::docs::{EndpointDoc, Example, endpoint},
    responses::{api_error, not_found},
};
use runinator_ws_middleware::authz::AuthzChecker;

async fn authorize_run<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    workflow_run_id: Uuid,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    AuthzChecker::new(db, ctx)
        .require_run_workflow(workflow_run_id, Permission::View)
        .await
}

pub async fn list_continuations<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    match db.fetch_workflow_continuations(workflow_run_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowContinuationList(records)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_continuation<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(continuation_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match db.fetch_workflow_continuation(continuation_id).await {
        Ok(Some(record)) => {
            if let Err(reply) = authorize_run(db.as_ref(), &ctx, record.workflow_run_id).await {
                return reply;
            }
            (
                StatusCode::OK,
                Json(ApiResponse::WorkflowContinuation(record)),
            )
        }
        Ok(None) => not_found(format!("workflow continuation {continuation_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_effects<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    match db.fetch_workflow_effects(workflow_run_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowEffectList(records)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_effect<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(effect_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match db.fetch_workflow_effect(effect_id).await {
        Ok(Some(record)) => {
            if let Err(reply) = authorize_run(db.as_ref(), &ctx, record.workflow_run_id).await {
                return reply;
            }
            (StatusCode::OK, Json(ApiResponse::WorkflowEffect(record)))
        }
        Ok(None) => not_found(format!("workflow effect {effect_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_journal<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    match db.fetch_workflow_journal(workflow_run_id).await {
        Ok(records) => (StatusCode::OK, Json(ApiResponse::WorkflowJournal(records))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_cursors<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    let module = match db.fetch_workflow_module(workflow_run_id).await {
        Ok(Some(module)) => module,
        Ok(None) => {
            return (
                StatusCode::OK,
                Json(ApiResponse::WorkflowVmCursors(Vec::new())),
            );
        }
        Err(err) => return api_error(err.to_string()),
    };
    match db.fetch_workflow_continuations(workflow_run_id).await {
        Ok(continuations) => {
            let cursors = continuations
                .into_iter()
                .map(|continuation| {
                    let location = module.graph_location(continuation.instruction_pointer);
                    WorkflowVmCursor {
                        continuation_id: continuation.id,
                        instruction_pointer: continuation.instruction_pointer,
                        node_id: location.map(|entry| entry.node_id.clone()),
                        edge_label: location.and_then(|entry| entry.edge_label.clone()),
                        status: continuation.status,
                    }
                })
                .collect();
            (
                StatusCode::OK,
                Json(ApiResponse::WorkflowVmCursors(cursors)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub fn routes<T: DatabaseImpl>(pool: Arc<T>) -> axum::Router {
    use axum::routing::get;
    axum::Router::new()
        .route(
            "/workflow_runs/{id}/continuations",
            get(list_continuations::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/effects",
            get(list_effects::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/journal",
            get(list_journal::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/cursors",
            get(list_cursors::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_continuations/{id}",
            get(get_continuation::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_effects/{id}",
            get(get_effect::<T>).layer(Extension(pool)),
        )
}

pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/workflow_runs/{id}/continuations",
        "Workflow VM",
        "List continuations",
        "Lists the durable branches of a VM-backed workflow run.",
        false,
        None,
        &[],
        200,
        "continuations",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/effects",
        "Workflow VM",
        "List effects",
        "Lists durable VM effects without reading node-run records.",
        false,
        None,
        &[],
        200,
        "effects",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/journal",
        "Workflow VM",
        "Read execution journal",
        "Returns the immutable VM execution history in sequence order.",
        false,
        None,
        &[],
        200,
        "journal",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/cursors",
        "Workflow VM",
        "Render graph cursors",
        "Projects continuation instruction pointers through the frozen module source map.",
        false,
        None,
        &[],
        200,
        "graph cursors",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_continuations/{id}",
        "Workflow VM",
        "Get continuation",
        "Returns one durable continuation by its execution identity.",
        false,
        None,
        &[],
        200,
        "continuation",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_effects/{id}",
        "Workflow VM",
        "Get effect",
        "Returns one durable VM effect by its identity.",
        false,
        None,
        &[],
        200,
        "effect",
        Example::WorkflowRun,
    ),
];
