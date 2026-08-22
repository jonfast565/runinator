//! Operator reads for the compiled workflow VM.
//!
//! These intentionally expose durable continuations, effects, and journal records directly.
//! A VM-backed run must never reconstruct its history from legacy node-run rows.

use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::{AuthContext, Permission},
    value::Value,
    web::TaskResponse,
    workflow_vm::WorkflowVmCursor,
    workflow_vm::{WorkflowEffect, WorkflowEffectStatus, WorkflowJournalEntry},
};
use serde::Deserialize;
use uuid::Uuid;

use runinator_ws_core::{
    events::{EventSender, emit_workflow_run},
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
    match project_effect_nodes(db.as_ref(), workflow_run_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowEffectList(records)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// Effects outlive the continuation location that issued them. Project their immutable journal
/// boundary through the pinned module so operator clients can keep historical node highlights.
async fn project_effect_nodes<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<Vec<WorkflowEffect>, runinator_models::errors::SendableError> {
    let (mut effects, journal, module) = tokio::try_join!(
        db.fetch_workflow_effects(workflow_run_id),
        db.fetch_workflow_journal(workflow_run_id),
        db.fetch_workflow_module(workflow_run_id),
    )?;
    let Some(module) = module else {
        return Ok(effects);
    };
    let node_by_effect = journal
        .into_iter()
        .filter_map(|record| match record.entry {
            WorkflowJournalEntry::EffectRequested {
                effect_id,
                instruction_pointer: Some(instruction_pointer),
            } => module
                .graph_location(instruction_pointer)
                .map(|location| (effect_id, location.node_id.clone())),
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    for effect in &mut effects {
        effect.node_id = node_by_effect.get(&effect.id).cloned();
    }
    Ok(effects)
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

pub async fn list_effect_output<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(effect_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let effect = match db.fetch_workflow_effect(effect_id).await {
        Ok(Some(effect)) => effect,
        Ok(None) => return not_found(format!("workflow effect {effect_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, effect.workflow_run_id).await {
        return reply;
    }
    match db.fetch_workflow_effect_output(effect_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowEffectOutput(records)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct SettleEffectRequest {
    pub status: WorkflowEffectStatus,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub message: Option<String>,
}

/// Resolve an approval/input/signal/gate/event wait by its durable effect identity. Provider
/// effects use the broker result path; accepting them here would bypass worker attempt ownership.
pub async fn settle_effect<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(effect_id): Path<Uuid>,
    Json(request): Json<SettleEffectRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let effect = match db.fetch_workflow_effect(effect_id).await {
        Ok(Some(effect)) => effect,
        Ok(None) => return not_found(format!("workflow effect {effect_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(effect.workflow_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    if matches!(
        effect.request,
        runinator_models::workflow_vm::WorkflowEffectRequest::Action { .. }
    ) {
        return runinator_ws_core::responses::bad_request(
            "provider effects can only be settled by their assigned worker",
        );
    }
    if !request.status.is_terminal() {
        return runinator_ws_core::responses::bad_request(
            "effect settlement status must be terminal",
        );
    }
    match db
        .settle_workflow_effect(
            effect_id,
            effect.attempt,
            request.status,
            request.output,
            request.message,
            chrono::Utc::now(),
        )
        .await
    {
        Ok(applied) => {
            if applied {
                let org_id =
                    crate::repository::org_id_for_workflow_run(db.as_ref(), effect.workflow_run_id)
                        .await;
                emit_workflow_run(&events, effect.workflow_run_id, org_id);
            }
            (
                StatusCode::OK,
                Json(ApiResponse::TaskResponse(TaskResponse {
                    success: applied,
                    message: if applied {
                        format!("Workflow effect {effect_id} settled")
                    } else {
                        format!("Workflow effect {effect_id} was already settled or stale")
                    },
                })),
            )
        }
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
                    // A suspended continuation has already advanced past the yielding opcode; the
                    // operator-facing cursor still belongs to the node that produced the effect.
                    let instruction_pointer = if continuation.awaiting_effect_id.is_some() {
                        continuation.instruction_pointer.saturating_sub(1)
                    } else {
                        continuation.instruction_pointer
                    };
                    let location = module.graph_location(instruction_pointer);
                    WorkflowVmCursor {
                        continuation_id: continuation.id,
                        instruction_pointer,
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
    use axum::routing::{get, post};
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
            get(get_effect::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_effects/{id}/output",
            get(list_effect_output::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_effects/{id}/settle",
            post(settle_effect::<T>).layer(Extension(pool)),
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
    endpoint(
        "get",
        "/workflow_effects/{id}/output",
        "Workflow VM",
        "List effect output",
        "Returns the durable output events recorded for one workflow VM effect.",
        false,
        None,
        &[],
        200,
        "effect output events",
        Example::WorkflowRun,
    ),
    endpoint(
        "post",
        "/workflow_effects/{id}/settle",
        "Workflow VM",
        "Settle effect",
        "Settles a non-provider workflow effect with a terminal status and optional output.",
        false,
        None,
        &[],
        200,
        "effect settlement result",
        Example::TaskResponse,
    ),
];
