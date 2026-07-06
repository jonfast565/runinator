use std::{net::SocketAddr, sync::Arc};
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::AuthContext,
    replicas::{
        ReplicaHeartbeatRequest, ReplicaOfflineRequest, ReplicaProviderRegistrationRequest,
        ReplicaRegistrationRequest,
    },
};

use crate::models::{ApiResponse, ReplicaQuery, ReplicaSampleQuery};
use crate::repository;
use crate::responses::{api_error, not_found};

pub(crate) async fn register_replica<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Json(request): Json<ReplicaRegistrationRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::register_replica(db.as_ref(), request, observed_ip(&headers, connect), &ctx)
        .await
    {
        Ok(replica) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn heartbeat_replica<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Path(replica_id): Path<Uuid>,
    Json(request): Json<ReplicaHeartbeatRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::heartbeat_replica(
        db.as_ref(),
        replica_id,
        request,
        observed_ip(&headers, connect),
    )
    .await
    {
        Ok(Some(replica)) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Ok(None) => not_found(format!(
            "Replica {replica_id} not found or runtime mismatch"
        )),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn mark_replica_offline<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    Json(request): Json<ReplicaOfflineRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::mark_replica_offline(db.as_ref(), replica_id, request.runtime_id).await {
        Ok(Some(replica)) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Ok(None) => not_found(format!(
            "Replica {replica_id} not found or runtime mismatch"
        )),
        Err(err) => api_error(err.to_string()),
    }
}

/// list service replicas in the cluster.
#[utoipa::path(
    get,
    path = "/replicas",
    tag = "Replicas",
    responses((status = 200, description = "service replicas", body = serde_json::Value)),
)]
pub(crate) async fn get_replicas<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(_ctx): Extension<AuthContext>,
    Query(query): Query<ReplicaQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_replicas(db.as_ref(), query.replica_type, query.status).await {
        Ok(replicas) => (StatusCode::OK, Json(ApiResponse::ReplicaList(replicas))),
        Err(err) => api_error(err.to_string()),
    }
}

/// fetch a replica's recent telemetry samples for charting.
pub(crate) async fn get_replica_samples<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(_ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    Query(query): Query<ReplicaSampleQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_replica_samples(db.as_ref(), replica_id, query.since_seconds).await {
        Ok(series) => (StatusCode::OK, Json(ApiResponse::ReplicaSamples(series))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn upsert_replica_provider<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    Json(request): Json<ReplicaProviderRegistrationRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::upsert_replica_provider_registration(db.as_ref(), replica_id, request).await {
        Ok(registration) => (
            StatusCode::OK,
            Json(ApiResponse::ReplicaProviderRegistration(registration)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_replica_providers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(_ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_replica_provider_registrations(db.as_ref(), replica_id).await {
        Ok(registrations) => (
            StatusCode::OK,
            Json(ApiResponse::ReplicaProviderRegistrationList(registrations)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

fn observed_ip(headers: &HeaderMap, connect: SocketAddr) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| Some(connect.ip().to_string()))
}
