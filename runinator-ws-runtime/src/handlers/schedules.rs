use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::{AuthContext, Permission},
    schedules::{BackfillRequest, NewFreezeWindow},
};
use serde::Deserialize;

use crate::repository;
use runinator_ws_core::events::{AppEvent, AppEventKind, EventSender, emit, nudge_wake_publisher};
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthzChecker};

type Reply = (StatusCode, Json<ApiResponse>);

#[derive(Deserialize, Default)]
pub struct FreezeWindowsQuery {
    /// narrow to one org's windows; the platform-wide ones are always included, since those are
    /// what actually freeze that org's schedules.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// list only the windows in effect right now.
    #[serde(default)]
    pub active: Option<bool>,
}

pub async fn list_freeze_windows<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<FreezeWindowsQuery>,
) -> Reply {
    let scope = query
        .org_id
        .and_then(|id| {
            runinator_models::rbac::ScopeRef::new(
                runinator_models::rbac::ScopeKind::Organization,
                Some(id),
            )
        })
        .unwrap_or(runinator_models::rbac::ScopeRef::PLATFORM);
    if let Err(reply) = ctx.require_scope_action(runinator_models::rbac::Action::View, scope) {
        return reply;
    }
    let windows = match query.active.unwrap_or(false) {
        true => repository::fetch_active_freeze_windows(db.as_ref()).await,
        false => repository::fetch_freeze_windows(db.as_ref(), query.org_id).await,
    };
    match windows {
        Ok(windows) => (StatusCode::OK, Json(ApiResponse::FreezeWindowList(windows))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_freeze_window<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut window): Json<NewFreezeWindow>,
) -> Reply {
    if let Err(reply) = require_window_target(db.as_ref(), &ctx, &window, Permission::Edit).await {
        return reply;
    }
    if let Some(workflow_id) = window.workflow_id {
        window.org_id = match repository::fetch_workflow(db.as_ref(), workflow_id).await {
            Ok(Some(workflow)) => workflow.org_id,
            Ok(None) => return not_found(format!("Workflow {workflow_id} not found")),
            Err(err) => return api_error(err.to_string()),
        };
    }
    match repository::create_freeze_window(db.as_ref(), &window).await {
        Ok(window) => {
            emit(&events, AppEvent::global(AppEventKind::SchedulesChanged));
            (StatusCode::CREATED, Json(ApiResponse::FreezeWindow(window)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_freeze_window<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(window_id): Path<Uuid>,
    Json(mut window): Json<NewFreezeWindow>,
) -> Reply {
    let current = match repository::fetch_freeze_window(db.as_ref(), window_id).await {
        Ok(Some(window)) => window,
        Ok(None) => return not_found(format!("Freeze window {window_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    let current_target = NewFreezeWindow {
        org_id: current.org_id,
        workflow_id: current.workflow_id,
        name: current.name,
        reason: current.reason,
        starts_at: current.starts_at,
        ends_at: current.ends_at,
        enabled: current.enabled,
    };
    if let Err(reply) =
        require_window_target(db.as_ref(), &ctx, &current_target, Permission::Edit).await
    {
        return reply;
    }
    if let Err(reply) = require_window_target(db.as_ref(), &ctx, &window, Permission::Edit).await {
        return reply;
    }
    if let Some(workflow_id) = window.workflow_id {
        window.org_id = match repository::fetch_workflow(db.as_ref(), workflow_id).await {
            Ok(Some(workflow)) => workflow.org_id,
            Ok(None) => return not_found(format!("Workflow {workflow_id} not found")),
            Err(err) => return api_error(err.to_string()),
        };
    }
    match repository::update_freeze_window(db.as_ref(), window_id, &window).await {
        Ok(Some(window)) => {
            emit(&events, AppEvent::global(AppEventKind::SchedulesChanged));
            (StatusCode::OK, Json(ApiResponse::FreezeWindow(window)))
        }
        Ok(None) => not_found(format!("Freeze window {window_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_freeze_window<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(window_id): Path<Uuid>,
) -> Reply {
    let current = match repository::fetch_freeze_window(db.as_ref(), window_id).await {
        Ok(Some(window)) => window,
        Ok(None) => return not_found(format!("Freeze window {window_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    let target = NewFreezeWindow {
        org_id: current.org_id,
        workflow_id: current.workflow_id,
        name: current.name,
        reason: current.reason,
        starts_at: current.starts_at,
        ends_at: current.ends_at,
        enabled: current.enabled,
    };
    if let Err(reply) = require_window_target(db.as_ref(), &ctx, &target, Permission::Edit).await {
        return reply;
    }
    match repository::delete_freeze_window(db.as_ref(), window_id).await {
        Ok(response) => {
            emit(&events, AppEvent::global(AppEventKind::SchedulesChanged));
            (StatusCode::OK, Json(ApiResponse::TaskResponse(response)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// replay a cron trigger's slots across a past range. slots the loop already fired keep their
/// original run, so re-issuing an overlapping backfill is safe.
pub async fn backfill_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
    Json(request): Json<BackfillRequest>,
) -> Reply {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_trigger_workflow(trigger_id, Permission::Edit)
        .await
    {
        return reply;
    }
    if let Err(err) = repository::validate_backfill_request(&request) {
        return api_error(err.to_string());
    }
    match repository::backfill_workflow_trigger(db.as_ref(), trigger_id, &request).await {
        Ok((response, runs)) => {
            for run in &runs {
                let org_id = repository::org_id_for_workflow_run(db.as_ref(), run.id).await;
                emit(
                    &events,
                    AppEvent::new(org_id, AppEventKind::WorkflowRunChanged { run_id: run.id }),
                );
            }
            // the backfilled runs have ready nodes waiting; do not make them sit out the wake
            // publisher's poll interval.
            if !runs.is_empty() {
                nudge_wake_publisher(&events);
            }
            (StatusCode::OK, Json(ApiResponse::Backfill(response)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn require_window_target<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    window: &NewFreezeWindow,
    needed: Permission,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    if let Some(workflow_id) = window.workflow_id {
        return AuthzChecker::new(db, ctx)
            .require_workflow(workflow_id, needed)
            .await;
    }
    let scope = window
        .org_id
        .and_then(|id| {
            runinator_models::rbac::ScopeRef::new(
                runinator_models::rbac::ScopeKind::Organization,
                Some(id),
            )
        })
        .unwrap_or(runinator_models::rbac::ScopeRef::PLATFORM);
    ctx.require_scope_action(runinator_models::rbac::Action::SchedulesManage, scope)
}

/// the `schedules` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, patch, post};
    axum::Router::new()
        .route(
            "/freeze_windows",
            get(list_freeze_windows::<T>)
                .post(create_freeze_window::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/freeze_windows/{id}",
            patch(update_freeze_window::<T>)
                .delete(delete_freeze_window::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/{id}/backfill",
            post(backfill_workflow_trigger::<T>).layer(Extension(pool.clone())),
        )
}
