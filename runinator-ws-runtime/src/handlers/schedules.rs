use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_models::{
    auth::{AuthContext, Permission},
    schedules::{BackfillRequest, NewFreezeWindow},
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, ScheduleStore},
};
use serde::Deserialize;

use runinator_engine::services::SchedulingOperations;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker};

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

pub async fn list_freeze_windows<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
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
    let windows = service
        .list_freeze_windows(query.org_id, query.active.unwrap_or(false))
        .await;
    match windows {
        Ok(windows) => (StatusCode::OK, Json(ApiResponse::FreezeWindowList(windows))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_freeze_window<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut window): Json<NewFreezeWindow>,
) -> Reply {
    if let Err(reply) = require_window_target(db.as_ref(), &ctx, &window, Permission::Edit).await {
        return reply;
    }
    if let Some(workflow_id) = window.workflow_id {
        window.org_id = match service.workflow(workflow_id).await {
            Ok(Some(workflow)) => workflow.org_id,
            Ok(None) => return not_found(format!("Workflow {workflow_id} not found")),
            Err(err) => return api_error(err.to_string()),
        };
    }
    match service.create_freeze_window(&window).await {
        Ok(window) => (StatusCode::CREATED, Json(ApiResponse::FreezeWindow(window))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_freeze_window<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(window_id): Path<Uuid>,
    Json(mut window): Json<NewFreezeWindow>,
) -> Reply {
    let current = match service.fetch_freeze_window(window_id).await {
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
        window.org_id = match service.workflow(workflow_id).await {
            Ok(Some(workflow)) => workflow.org_id,
            Ok(None) => return not_found(format!("Workflow {workflow_id} not found")),
            Err(err) => return api_error(err.to_string()),
        };
    }
    match service.update_freeze_window(window_id, &window).await {
        Ok(Some(window)) => (StatusCode::OK, Json(ApiResponse::FreezeWindow(window))),
        Ok(None) => not_found(format!("Freeze window {window_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_freeze_window<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(window_id): Path<Uuid>,
) -> Reply {
    let current = match service.fetch_freeze_window(window_id).await {
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
    match service.delete_freeze_window(window_id).await {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::TaskResponse(response))),
        Err(err) => api_error(err.to_string()),
    }
}

/// replay a cron trigger's slots across a past range. slots the loop already fired keep their
/// original run, so re-issuing an overlapping backfill is safe.
pub async fn backfill_workflow_trigger<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
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
    if let Err(err) = service.validate_backfill(&request) {
        return api_error(err.to_string());
    }
    match service
        .backfill_workflow_trigger(trigger_id, &request)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::Backfill(response))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn require_window_target<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
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
pub fn routes<T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
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
