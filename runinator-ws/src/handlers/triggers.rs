use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::workflows::WorkflowTrigger;

use crate::events::{AppEvent, EventSender, emit};
use crate::models::{ApiResponse, SchedulerTriggerClaimRequest};
use crate::repository;
use crate::responses::{api_error, not_found};

pub(crate) async fn upsert_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_id): Path<i64>,
    Json(mut trigger): Json<WorkflowTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    trigger.workflow_id = workflow_id;
    match repository::upsert_workflow_trigger(db.as_ref(), &trigger).await {
        Ok(trigger) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn update_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(trigger_id): Path<i64>,
    Json(mut trigger): Json<WorkflowTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    trigger.id = Some(trigger_id);
    match repository::upsert_workflow_trigger(db.as_ref(), &trigger).await {
        Ok(trigger) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(trigger_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_trigger(db.as_ref(), trigger_id).await {
        Ok(Some(trigger)) => (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger))),
        Ok(None) => not_found(format!("Workflow trigger {trigger_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_workflow_triggers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_triggers(db.as_ref(), workflow_id).await {
        Ok(triggers) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowTriggerList(triggers)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_due_workflow_triggers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_due_workflow_triggers(db.as_ref()).await {
        Ok(triggers) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowTriggerList(triggers)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn claim_due_workflow_trigger_firings<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(request): Json<SchedulerTriggerClaimRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::claim_due_workflow_trigger_firings(
        db.as_ref(),
        request.scheduler_id,
        request.limit.unwrap_or(50),
    )
    .await
    {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(trigger_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::delete_workflow_trigger(db.as_ref(), trigger_id).await {
        Ok(resp) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}
