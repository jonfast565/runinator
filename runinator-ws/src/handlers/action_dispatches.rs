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

use crate::responses::api_error;

#[derive(Debug, Deserialize)]
pub(crate) struct EnqueueActionDispatchRequest {
    pub dedupe_key: String,
    pub command: ActionCommand,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PendingActionDispatchQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ActionDispatchFailureRequest {
    pub error: String,
}

pub(crate) async fn enqueue_action_dispatch<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<EnqueueActionDispatchRequest>,
) -> Result<(StatusCode, Json<ActionDispatchRecord>), (StatusCode, Json<crate::models::ApiResponse>)>
{
    crate::authz::require_service_or_admin(&ctx)?;
    db.enqueue_action_dispatch(request.dedupe_key, request.command)
        .await
        .map(|record| (StatusCode::ACCEPTED, Json(record)))
        .map_err(|err| api_error(err.to_string()))
}

pub(crate) async fn pending_action_dispatches<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<PendingActionDispatchQuery>,
) -> Result<Json<Vec<ActionDispatchRecord>>, (StatusCode, Json<crate::models::ApiResponse>)> {
    crate::authz::require_service_or_admin(&ctx)?;
    db.fetch_pending_action_dispatches(query.limit.unwrap_or(100))
        .await
        .map(Json)
        .map_err(|err| api_error(err.to_string()))
}

pub(crate) async fn claim_action_dispatches<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<ActionDispatchClaimRequest>,
) -> Result<Json<Vec<ActionDispatchRecord>>, (StatusCode, Json<crate::models::ApiResponse>)> {
    crate::authz::require_service_or_admin(&ctx)?;
    db.claim_pending_action_dispatches(
        request.scheduler_id,
        chrono::Utc::now(),
        request.lease_until,
        request.limit.unwrap_or(100),
    )
    .await
    .map(Json)
    .map_err(|err| api_error(err.to_string()))
}

pub(crate) async fn mark_action_dispatch_published<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(dispatch_id): Path<Uuid>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<crate::models::ApiResponse>)> {
    crate::authz::require_service_or_admin(&ctx)?;
    db.mark_action_dispatch_published(dispatch_id)
        .await
        .map(|_| Json(success("Action dispatch marked published")))
        .map_err(|err| api_error(err.to_string()))
}

pub(crate) async fn mark_action_dispatch_failed<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(dispatch_id): Path<Uuid>,
    Json(request): Json<ActionDispatchFailureRequest>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<crate::models::ApiResponse>)> {
    crate::authz::require_service_or_admin(&ctx)?;
    db.mark_action_dispatch_failed(dispatch_id, request.error)
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
