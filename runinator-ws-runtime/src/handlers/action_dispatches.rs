use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::web::TaskResponse;
use runinator_models::{auth::AuthContext, orchestration::ActionDispatchClaimRequest};
use serde::Deserialize;

use crate::repository;
use runinator_models::rbac::SystemRole;
use runinator_ws_core::openapi::docs::{
    EndpointDoc, EndpointPolicy, Example, endpoint_with_policy, json_body,
};
use runinator_ws_core::responses::api_error;
use runinator_ws_middleware::authz::AuthContextExt;

#[derive(Debug, Deserialize)]
pub struct EnqueueActionDispatchRequest {
    pub dedupe_key: String,
    pub command: ActionCommand,
}

#[derive(Debug, Deserialize)]
pub struct PendingActionDispatchQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ActionDispatchFailureRequest {
    pub error: String,
}

pub async fn enqueue_action_dispatch<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<EnqueueActionDispatchRequest>,
) -> Result<
    (StatusCode, Json<ActionDispatchRecord>),
    (StatusCode, Json<runinator_ws_core::models::ApiResponse>),
> {
    ctx.require_system_role(&[runinator_models::rbac::SystemRole::Engine])?;
    repository::enqueue_action_dispatch(db.as_ref(), request.dedupe_key, request.command)
        .await
        .map(|record| (StatusCode::ACCEPTED, Json(record)))
        .map_err(|err| api_error(err.to_string()))
}

pub async fn pending_action_dispatches<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<PendingActionDispatchQuery>,
) -> Result<
    Json<Vec<ActionDispatchRecord>>,
    (StatusCode, Json<runinator_ws_core::models::ApiResponse>),
> {
    ctx.require_system_role(&[runinator_models::rbac::SystemRole::Engine])?;
    repository::fetch_pending_action_dispatches(db.as_ref(), query.limit.unwrap_or(100))
        .await
        .map(Json)
        .map_err(|err| api_error(err.to_string()))
}

pub async fn claim_action_dispatches<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<ActionDispatchClaimRequest>,
) -> Result<
    Json<Vec<ActionDispatchRecord>>,
    (StatusCode, Json<runinator_ws_core::models::ApiResponse>),
> {
    ctx.require_system_role(&[runinator_models::rbac::SystemRole::Engine])?;
    repository::claim_pending_action_dispatches(
        db.as_ref(),
        request.scheduler_id,
        request.lease_until,
        request.limit.unwrap_or(100),
    )
    .await
    .map(Json)
    .map_err(|err| api_error(err.to_string()))
}

pub async fn mark_action_dispatch_published<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(dispatch_id): Path<Uuid>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<runinator_ws_core::models::ApiResponse>)> {
    ctx.require_system_role(&[runinator_models::rbac::SystemRole::Engine])?;
    repository::mark_action_dispatch_published(db.as_ref(), dispatch_id)
        .await
        .map(|_| Json(success("Action dispatch marked published")))
        .map_err(|err| api_error(err.to_string()))
}

pub async fn mark_action_dispatch_failed<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(dispatch_id): Path<Uuid>,
    Json(request): Json<ActionDispatchFailureRequest>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<runinator_ws_core::models::ApiResponse>)> {
    ctx.require_system_role(&[runinator_models::rbac::SystemRole::Engine])?;
    repository::mark_action_dispatch_failed(db.as_ref(), dispatch_id, request.error)
        .await
        .map(|_| Json(success("Action dispatch failure recorded")))
        .map_err(|err| api_error(err.to_string()))
}

fn success(message: impl Into<String>) -> TaskResponse {
    TaskResponse {
        success: true,
        message: message.into(),
    }
}

/// the `action_dispatches` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_SCHEDULER_ACTION_DISPATCHES,
            post(enqueue_action_dispatch::<T>).layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_SCHEDULER_ACTION_DISPATCHES_PENDING,
            get(pending_action_dispatches::<T>).layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_SCHEDULER_ACTION_DISPATCHES_CLAIM,
            post(claim_action_dispatches::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/action_dispatches/{id}/published",
            post(mark_action_dispatch_published::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/action_dispatches/{id}/failed",
            post(mark_action_dispatch_failed::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint_with_policy(
        "post",
        "/scheduler/action_dispatches",
        "Control Plane",
        "Enqueue an action dispatch",
        "Durable outbox endpoint for scheduling an action command that a worker will execute.",
        EndpointPolicy::SystemRole(&[SystemRole::Engine]),
        json_body("Action dispatch record.", Example::ActionDispatch),
        &[],
        200,
        "action dispatch queued",
        Example::ActionDispatch,
    ),
    endpoint_with_policy(
        "get",
        "/scheduler/action_dispatches/pending",
        "Control Plane",
        "List pending action dispatches",
        "Lists action dispatches waiting to be published to the broker action channel.",
        EndpointPolicy::SystemRole(&[SystemRole::Engine]),
        None,
        &[],
        200,
        "pending action dispatches",
        Example::ActionDispatchList,
    ),
    endpoint_with_policy(
        "post",
        "/scheduler/action_dispatches/claim",
        "Control Plane",
        "Claim action dispatches",
        "Claims pending action dispatches for the action publisher loop.",
        EndpointPolicy::SystemRole(&[SystemRole::Engine]),
        json_body("Claim owner and limit.", Example::ActionDispatch),
        &[],
        200,
        "claimed action dispatches",
        Example::ActionDispatchList,
    ),
    endpoint_with_policy(
        "post",
        "/scheduler/action_dispatches/{id}/published",
        "Control Plane",
        "Mark action dispatch published",
        "Marks an action dispatch as successfully published to the broker.",
        EndpointPolicy::SystemRole(&[SystemRole::Engine]),
        None,
        &[],
        200,
        "action dispatch marked published",
        Example::TaskResponse,
    ),
    endpoint_with_policy(
        "post",
        "/scheduler/action_dispatches/{id}/failed",
        "Control Plane",
        "Mark action dispatch failed",
        "Records a publish failure for an action dispatch.",
        EndpointPolicy::SystemRole(&[SystemRole::Engine]),
        json_body("Failure detail.", Example::ActionDispatch),
        &[],
        200,
        "action dispatch marked failed",
        Example::TaskResponse,
    ),
];
