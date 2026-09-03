use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_models::{
    auth::{AuthContext, Permission, ResourceType},
    notifications::{NewNotification, NewNotificationPolicy, Notification},
    web::TaskResponse,
};
use runinator_store::{RuntimeStore, roles::NotificationStore};
use serde::Deserialize;

use runinator_engine::services::NotificationOperations;
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker, IntoReply};

type Reply = (StatusCode, Json<ApiResponse>);

async fn notification_visible<T: AuthorizationStore>(
    db: &T,
    ctx: &AuthContext,
    notification: &Notification,
) -> Result<bool, Reply> {
    match (
        notification.source_resource_type,
        notification.source_resource_id,
    ) {
        (Some(resource_type), Some(resource_id)) => Ok(AuthzChecker::new(db, ctx)
            .resource_permission(resource_type, resource_id)
            .await?
            .is_some_and(|permission| permission.allows(Permission::View))),
        (None, None) => {
            Ok(ctx.authorize_scope(runinator_models::rbac::Action::View, ctx.selected_scope()))
        }
        _ => Ok(false),
    }
}

fn forbidden(message: &str) -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::ApiError(
            runinator_ws_core::models::ApiError::new(message),
        )),
    )
}

#[derive(Deserialize, Default)]
pub struct NotificationsListQuery {
    #[serde(default)]
    pub unread: Option<bool>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub async fn list_notifications<T: AuthorizationStore + RuntimeStore + NotificationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<NotificationsListQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    let unread_only = query.unread.unwrap_or(false);
    let limit = query.limit.unwrap_or(200);
    let Some(user_id) = ctx.principal_id else {
        return forbidden("notifications require a user principal");
    };
    match service.list(ctx.org_id, user_id, unread_only, 1000).await {
        Ok(notifications) => {
            let mut visible = Vec::with_capacity(notifications.len());
            for notification in notifications {
                match notification_visible(db.as_ref(), &ctx, &notification).await {
                    Ok(true) => visible.push(notification),
                    Ok(false) => {}
                    Err(reply) => return reply.into_reply(),
                }
            }
            visible.truncate(limit.clamp(1, 1000) as usize);
            (StatusCode::OK, Json(ApiResponse::NotificationList(visible)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_notification<T: AuthorizationStore + RuntimeStore + NotificationStore>(
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(notification): ValidatedJson<NewNotification>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[runinator_models::rbac::SystemRole::Engine]) {
        return reply.into_reply();
    }
    match service.create(&notification).await {
        Ok(notification) => (
            StatusCode::CREATED,
            Json(ApiResponse::Notification(notification)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn mark_notification_read<T: AuthorizationStore + RuntimeStore + NotificationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(notification_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    let Some(user_id) = ctx.principal_id else {
        return forbidden("notifications require a user principal");
    };
    let notification = match service.fetch(ctx.org_id, notification_id, user_id).await {
        Ok(Some(notification)) => notification,
        Ok(None) => return not_found(format!("Notification {notification_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if !matches!(
        notification_visible(db.as_ref(), &ctx, &notification).await,
        Ok(true)
    ) {
        return not_found(format!("Notification {notification_id} not found"));
    }
    match service
        .mark_read(ctx.org_id, notification_id, user_id)
        .await
    {
        Ok(Some(notification)) => (
            StatusCode::OK,
            Json(ApiResponse::Notification(notification)),
        ),
        Ok(None) => not_found(format!("Notification {notification_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_notification<T: AuthorizationStore + RuntimeStore + NotificationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(notification_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    let Some(user_id) = ctx.principal_id else {
        return forbidden("notifications require a user principal");
    };
    let notification = match service.fetch(ctx.org_id, notification_id, user_id).await {
        Ok(Some(notification)) => notification,
        Ok(None) => return not_found(format!("Notification {notification_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if !matches!(
        notification_visible(db.as_ref(), &ctx, &notification).await,
        Ok(true)
    ) {
        return not_found(format!("Notification {notification_id} not found"));
    }
    match service.delete(ctx.org_id, notification_id, user_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: true,
                message: "Notification dismissed from my inbox".to_string(),
            })),
        ),
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

pub async fn list_notification_policies<
    T: AuthorizationStore + RuntimeStore + NotificationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<NotificationPoliciesQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(workflow_id) = query.workflow_id {
        if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_workflow(workflow_id, Permission::View)
            .await
        {
            return reply.into_reply();
        }
    } else if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    match service.list_policies(ctx.org_id, query.workflow_id).await {
        Ok(mut policies) => {
            let mut visible = Vec::with_capacity(policies.len());
            for policy in policies.drain(..) {
                let permission = match policy.workflow_id {
                    Some(workflow_id) => {
                        AuthzChecker::new(db.as_ref(), &ctx)
                            .resource_permission(ResourceType::Workflow, workflow_id)
                            .await
                    }
                    None if policy.org_id == ctx.org_id => {
                        AuthzChecker::new(db.as_ref(), &ctx)
                            .resource_permission(ResourceType::NotificationPolicy, policy.id)
                            .await
                    }
                    None => continue,
                };
                match permission {
                    Ok(Some(permission)) if permission.allows(Permission::View) => {
                        visible.push(policy)
                    }
                    Ok(_) => {}
                    Err(reply) => return reply.into_reply(),
                }
            }
            (
                StatusCode::OK,
                Json(ApiResponse::NotificationPolicyList(visible)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_notification_policy<
    T: AuthorizationStore + RuntimeStore + NotificationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(mut policy): ValidatedJson<NewNotificationPolicy>,
) -> Reply {
    policy.org_id = ctx.org_id;
    if let Err(reply) =
        require_policy_target(db.as_ref(), &ctx, policy.workflow_id, Permission::Edit).await
    {
        return reply.into_reply();
    }
    match service.create_policy(&policy).await {
        Ok(policy) => {
            if policy.workflow_id.is_none()
                && let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                    .grant_resource_owner(
                        runinator_models::auth::ResourceType::NotificationPolicy,
                        policy.id,
                    )
                    .await
            {
                return reply.into_reply();
            }
            (
                StatusCode::CREATED,
                Json(ApiResponse::NotificationPolicy(policy)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_notification_policy<
    T: AuthorizationStore + RuntimeStore + NotificationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(policy_id): Path<Uuid>,
    ValidatedJson(mut policy): ValidatedJson<NewNotificationPolicy>,
) -> Reply {
    let current = match service.fetch_policy(policy_id).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return not_found(format!("Notification policy {policy_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    policy.org_id = current.org_id;
    let current_access = match current.workflow_id {
        Some(workflow_id) => {
            AuthzChecker::new(db.as_ref(), &ctx)
                .require_workflow(workflow_id, Permission::Edit)
                .await
        }
        None => {
            AuthzChecker::new(db.as_ref(), &ctx)
                .require_resource(
                    ResourceType::NotificationPolicy,
                    policy_id,
                    Permission::Edit,
                )
                .await
        }
    };
    if let Err(reply) = current_access {
        return reply.into_reply();
    }
    if policy.workflow_id != current.workflow_id {
        return bad_request(
            "a notification policy cannot switch between standalone and workflow-specific scope",
        );
    }
    if let Err(reply) =
        require_policy_target(db.as_ref(), &ctx, policy.workflow_id, Permission::Edit).await
    {
        return reply.into_reply();
    }
    match service.update_policy(policy_id, &policy).await {
        Ok(Some(policy)) => (
            StatusCode::OK,
            Json(ApiResponse::NotificationPolicy(policy)),
        ),
        Ok(None) => not_found(format!("Notification policy {policy_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_notification_policy<
    T: AuthorizationStore + RuntimeStore + NotificationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(policy_id): Path<Uuid>,
) -> Reply {
    let current = match service.fetch_policy(policy_id).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return not_found(format!("Notification policy {policy_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    let access = match current.workflow_id {
        Some(workflow_id) => {
            AuthzChecker::new(db.as_ref(), &ctx)
                .require_workflow(workflow_id, Permission::Own)
                .await
        }
        None => {
            AuthzChecker::new(db.as_ref(), &ctx)
                .require_resource(ResourceType::NotificationPolicy, policy_id, Permission::Own)
                .await
        }
    };
    if let Err(reply) = access {
        return reply.into_reply();
    }
    match service.delete_policy(policy_id).await {
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

async fn require_policy_target<T: AuthorizationStore + RuntimeStore + NotificationStore>(
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
        None => ctx
            .require_scope_action(
                runinator_models::rbac::Action::NotificationsManage,
                ctx.selected_scope(),
            )
            .map_err(IntoReply::into_reply),
    }
}

/// the external-channel delivery attempts for one notification, so an operator can see whether the
/// alert actually reached slack/email rather than only that it was raised.
pub async fn list_notification_deliveries<
    T: AuthorizationStore + RuntimeStore + NotificationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(notification_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let Some(user_id) = ctx.principal_id else {
        return forbidden("notifications require a user principal");
    };
    let notification = match service.fetch(ctx.org_id, notification_id, user_id).await {
        Ok(Some(notification)) => notification,
        Ok(None) => return not_found(format!("Notification {notification_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if !matches!(
        notification_visible(db.as_ref(), &ctx, &notification).await,
        Ok(true)
    ) {
        return not_found(format!("Notification {notification_id} not found"));
    }
    match service.deliveries(notification_id).await {
        Ok(deliveries) => (
            StatusCode::OK,
            Json(ApiResponse::NotificationDeliveryList(deliveries)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn mark_all_notifications_read<
    T: AuthorizationStore + RuntimeStore + NotificationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<NotificationOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    let Some(user_id) = ctx.principal_id else {
        return forbidden("notifications require a user principal");
    };
    let notifications = match service.list(ctx.org_id, user_id, true, 1000).await {
        Ok(notifications) => notifications,
        Err(err) => return api_error(err.to_string()),
    };
    let mut count = 0;
    for notification in notifications {
        match notification_visible(db.as_ref(), &ctx, &notification).await {
            Ok(true) => match service
                .mark_read(ctx.org_id, notification.id, user_id)
                .await
            {
                Ok(Some(_)) => count += 1,
                Ok(None) => {}
                Err(err) => return api_error(err.to_string()),
            },
            Ok(false) => {}
            Err(reply) => return reply.into_reply(),
        }
    }
    (
        StatusCode::OK,
        Json(ApiResponse::TaskResponse(TaskResponse {
            success: true,
            message: format!("Marked {count} notification(s) as read"),
        })),
    )
}

/// the `notifications` endpoints.
pub fn routes<T: AuthorizationStore + RuntimeStore + NotificationStore>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
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
    endpoint!(
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
    endpoint!(
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
    endpoint!(
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
    endpoint!(
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
