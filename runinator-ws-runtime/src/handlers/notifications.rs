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
    notifications::{NewNotification, NewNotificationPolicy},
    web::TaskResponse,
};
use serde::Deserialize;

use runinator_engine::repository;
use runinator_ws_core::events::{AppEvent, AppEventKind, EventSender, emit};
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthzChecker};

type Reply = (StatusCode, Json<ApiResponse>);

#[derive(Deserialize, Default)]
pub struct NotificationsListQuery {
    #[serde(default)]
    pub unread: Option<bool>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub async fn list_notifications<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<NotificationsListQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply;
    }
    let unread_only = query.unread.unwrap_or(false);
    let limit = query.limit.unwrap_or(200);
    match repository::fetch_notifications(db.as_ref(), unread_only, limit).await {
        Ok(notifications) => (
            StatusCode::OK,
            Json(ApiResponse::NotificationList(notifications)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_notification<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Json(notification): Json<NewNotification>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[runinator_models::rbac::SystemRole::Engine]) {
        return reply;
    }
    match repository::create_notification(db.as_ref(), &notification).await {
        Ok(created) => {
            emit(
                &events,
                AppEvent::new(
                    created.org_id,
                    AppEventKind::NotificationCreated {
                        notification_id: created.notification.id,
                    },
                ),
            );
            (
                StatusCode::CREATED,
                Json(ApiResponse::Notification(created.notification)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn mark_notification_read<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(notification_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::NotificationsManage,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    match repository::mark_notification_read(db.as_ref(), notification_id).await {
        Ok(Some(notification)) => {
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

pub async fn delete_notification<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(notification_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::NotificationsManage,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    match repository::delete_notification(db.as_ref(), notification_id).await {
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

#[derive(Deserialize, Default)]
pub struct NotificationPoliciesQuery {
    /// narrow to one workflow's own policies; omit for every policy including the global ones.
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
}

pub async fn list_notification_policies<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<NotificationPoliciesQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(workflow_id) = query.workflow_id {
        if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_workflow(workflow_id, Permission::View)
            .await
        {
            return reply;
        }
    } else if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::View,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    match repository::fetch_notification_policies(db.as_ref(), query.workflow_id).await {
        Ok(policies) => (
            StatusCode::OK,
            Json(ApiResponse::NotificationPolicyList(policies)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_notification_policy<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(policy): Json<NewNotificationPolicy>,
) -> Reply {
    if let Err(reply) =
        require_policy_target(db.as_ref(), &ctx, policy.workflow_id, Permission::Edit).await
    {
        return reply;
    }
    match repository::create_notification_policy(db.as_ref(), &policy).await {
        Ok(policy) => (
            StatusCode::CREATED,
            Json(ApiResponse::NotificationPolicy(policy)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_notification_policy<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(policy_id): Path<Uuid>,
    Json(policy): Json<NewNotificationPolicy>,
) -> Reply {
    let current = match repository::fetch_notification_policy(db.as_ref(), policy_id).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return not_found(format!("Notification policy {policy_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) =
        require_policy_target(db.as_ref(), &ctx, current.workflow_id, Permission::Edit).await
    {
        return reply;
    }
    if let Err(reply) =
        require_policy_target(db.as_ref(), &ctx, policy.workflow_id, Permission::Edit).await
    {
        return reply;
    }
    match repository::update_notification_policy(db.as_ref(), policy_id, &policy).await {
        Ok(Some(policy)) => (
            StatusCode::OK,
            Json(ApiResponse::NotificationPolicy(policy)),
        ),
        Ok(None) => not_found(format!("Notification policy {policy_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_notification_policy<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(policy_id): Path<Uuid>,
) -> Reply {
    let current = match repository::fetch_notification_policy(db.as_ref(), policy_id).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return not_found(format!("Notification policy {policy_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) =
        require_policy_target(db.as_ref(), &ctx, current.workflow_id, Permission::Own).await
    {
        return reply;
    }
    match repository::delete_notification_policy(db.as_ref(), policy_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: true,
                message: "Notification policy deleted".to_string(),
            })),
        ),
        Ok(false) => not_found(format!("Notification policy {policy_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

async fn require_policy_target<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    workflow_id: Option<Uuid>,
    needed: Permission,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    match workflow_id {
        Some(workflow_id) => {
            AuthzChecker::new(db, ctx)
                .require_workflow(workflow_id, needed)
                .await
        }
        None => ctx.require_scope_action(
            runinator_models::rbac::Action::NotificationsManage,
            runinator_models::rbac::ScopeRef::PLATFORM,
        ),
    }
}

/// the external-channel delivery attempts for one notification, so an operator can see whether the
/// alert actually reached slack/email rather than only that it was raised.
pub async fn list_notification_deliveries<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(notification_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_notification_deliveries(db.as_ref(), notification_id).await {
        Ok(deliveries) => (
            StatusCode::OK,
            Json(ApiResponse::NotificationDeliveryList(deliveries)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn mark_all_notifications_read<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::mark_all_notifications_read(db.as_ref()).await {
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

/// the `notifications` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{delete, get, patch, post};
    axum::Router::new()
        .route(
            "/notifications",
            get(list_notifications::<T>)
                .post(create_notification::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/notifications/{id}",
            delete(delete_notification::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/notifications/{id}/mark_read",
            post(mark_notification_read::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/notifications/mark_all_read",
            post(mark_all_notifications_read::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/notifications/{id}/deliveries",
            get(list_notification_deliveries::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/notification_policies",
            get(list_notification_policies::<T>)
                .post(create_notification_policy::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/notification_policies/{id}",
            patch(update_notification_policy::<T>)
                .delete(delete_notification_policy::<T>)
                .layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/notifications",
        "Notifications",
        "List notifications",
        "Lists notifications for the current principal.",
        false,
        None,
        &[],
        200,
        "notifications",
        Example::NotificationList,
    ),
    endpoint(
        "post",
        "/notifications",
        "Notifications",
        "Create a notification",
        "Creates a notification record.",
        false,
        json_body("Notification payload.", Example::Notification),
        &[],
        200,
        "created notification",
        Example::Notification,
    ),
    endpoint(
        "post",
        "/notifications/{id}/mark_read",
        "Notifications",
        "Mark a notification read",
        "Marks one notification as read.",
        false,
        None,
        &[],
        200,
        "notification marked read",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/notifications/mark_all_read",
        "Notifications",
        "Mark all notifications read",
        "Marks all notifications visible to the caller as read.",
        false,
        None,
        &[],
        200,
        "notifications marked read",
        Example::TaskResponse,
    ),
];
