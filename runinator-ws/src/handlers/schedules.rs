use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::AuthContext,
    capabilities::Capability,
    schedules::{BackfillRequest, NewFreezeWindow},
};
use serde::Deserialize;

use crate::authz;
use crate::events::{AppEvent, AppEventKind, EventSender, emit, nudge_wake_publisher};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, not_found};

type Reply = (StatusCode, Json<ApiResponse>);

#[derive(Deserialize, Default)]
pub(crate) struct FreezeWindowsQuery {
    /// narrow to one org's windows; the platform-wide ones are always included, since those are
    /// what actually freeze that org's schedules.
    #[serde(default)]
    pub(crate) org_id: Option<Uuid>,
    /// list only the windows in effect right now.
    #[serde(default)]
    pub(crate) active: Option<bool>,
}

pub(crate) async fn list_freeze_windows<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<FreezeWindowsQuery>,
) -> Reply {
    let windows = match query.active.unwrap_or(false) {
        true => repository::fetch_active_freeze_windows(db.as_ref()).await,
        false => repository::fetch_freeze_windows(db.as_ref(), query.org_id).await,
    };
    match windows {
        Ok(windows) => (StatusCode::OK, Json(ApiResponse::FreezeWindowList(windows))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_freeze_window<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Json(window): Json<NewFreezeWindow>,
) -> Reply {
    if let Err(reply) = authz::require_capability(&ctx, Capability::SchedulesManage) {
        return reply;
    }
    match repository::create_freeze_window(db.as_ref(), &window).await {
        Ok(window) => {
            emit(&events, AppEvent::global(AppEventKind::SchedulesChanged));
            (StatusCode::CREATED, Json(ApiResponse::FreezeWindow(window)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn update_freeze_window<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(window_id): Path<Uuid>,
    Json(window): Json<NewFreezeWindow>,
) -> Reply {
    if let Err(reply) = authz::require_capability(&ctx, Capability::SchedulesManage) {
        return reply;
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

pub(crate) async fn delete_freeze_window<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(window_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = authz::require_capability(&ctx, Capability::SchedulesManage) {
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
pub(crate) async fn backfill_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
    Json(request): Json<BackfillRequest>,
) -> Reply {
    if let Err(reply) = authz::require_capability(&ctx, Capability::SchedulesManage) {
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
