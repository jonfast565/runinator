use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{notifications::NewNotification, web::TaskResponse};
use serde::Deserialize;

use crate::events::{AppEvent, AppEventKind, EventSender, emit};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, not_found};

#[derive(Deserialize, Default)]
pub(crate) struct NotificationsListQuery {
    #[serde(default)]
    pub(crate) unread: Option<bool>,
    #[serde(default)]
    pub(crate) limit: Option<i64>,
}

pub(crate) async fn list_notifications<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<NotificationsListQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let unread_only = query.unread.unwrap_or(false);
    let limit = query.limit.unwrap_or(200);
    match db.fetch_notifications(unread_only, limit).await {
        Ok(notifications) => (
            StatusCode::OK,
            Json(ApiResponse::NotificationList(notifications)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_notification<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Json(notification): Json<NewNotification>,
) -> (StatusCode, Json<ApiResponse>) {
    match db.create_notification(&notification).await {
        Ok(notification) => {
            let org_id = if let Some(workflow_run_id) = notification.workflow_run_id {
                repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await
            } else {
                None
            };
            emit(
                &events,
                AppEvent::new(
                    org_id,
                    AppEventKind::NotificationCreated {
                        notification_id: notification.id,
                    },
                ),
            );
            (
                StatusCode::CREATED,
                Json(ApiResponse::Notification(notification)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn mark_notification_read<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(notification_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match db.mark_notification_read(notification_id).await {
        Ok(Some(notification)) => {
            // no auth ctx on this handler — leave global.
            emit(
                &events,
                AppEvent::global(AppEventKind::NotificationsChanged),
            );
            (
                StatusCode::OK,
                Json(ApiResponse::Notification(notification)),
            )
        }
        Ok(None) => not_found(format!("Notification {notification_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_notification<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(notification_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match db.delete_notification(notification_id).await {
        Ok(true) => {
            emit(
                &events,
                AppEvent::global(AppEventKind::NotificationsChanged),
            );
            (
                StatusCode::OK,
                Json(ApiResponse::TaskResponse(TaskResponse {
                    success: true,
                    message: "Notification deleted".to_string(),
                })),
            )
        }
        Ok(false) => not_found(format!("Notification {notification_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn mark_all_notifications_read<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
) -> (StatusCode, Json<ApiResponse>) {
    match db.mark_all_notifications_read().await {
        Ok(count) => {
            emit(
                &events,
                AppEvent::global(AppEventKind::NotificationsChanged),
            );
            (
                StatusCode::OK,
                Json(ApiResponse::TaskResponse(TaskResponse {
                    success: true,
                    message: format!("Marked {count} notification(s) as read"),
                })),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}
