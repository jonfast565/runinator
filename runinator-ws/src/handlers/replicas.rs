use std::{net::SocketAddr, sync::Arc};

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaOfflineRequest, ReplicaProviderRegistrationRequest,
    ReplicaRegistrationRequest,
};

use crate::models::{ApiResponse, ReplicaQuery};
use crate::repository;
use crate::responses::{api_error, not_found};

pub(crate) async fn register_replica<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Json(request): Json<ReplicaRegistrationRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::register_replica(
        db.as_ref(),
        request,
        observed_ip(&headers, connect),
    )
    .await
    {
        Ok(replica) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn heartbeat_replica<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Path(replica_id): Path<i64>,
    Json(request): Json<ReplicaHeartbeatRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::heartbeat_replica(
        db.as_ref(),
        replica_id,
        request,
        observed_ip(&headers, connect),
    )
    .await
    {
        Ok(Some(replica)) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Ok(None) => not_found(format!("Replica {replica_id} not found or runtime mismatch")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn mark_replica_offline<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(replica_id): Path<i64>,
    Json(request): Json<ReplicaOfflineRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::mark_replica_offline(db.as_ref(), replica_id, request.runtime_id).await {
        Ok(Some(replica)) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Ok(None) => not_found(format!("Replica {replica_id} not found or runtime mismatch")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_replicas<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<ReplicaQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_replicas(db.as_ref(), query.replica_type, query.status).await {
        Ok(replicas) => (StatusCode::OK, Json(ApiResponse::ReplicaList(replicas))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn upsert_replica_provider<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(replica_id): Path<i64>,
    Json(request): Json<ReplicaProviderRegistrationRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::upsert_replica_provider_registration(db.as_ref(), replica_id, request).await
    {
        Ok(registration) => (
            StatusCode::OK,
            Json(ApiResponse::ReplicaProviderRegistration(registration)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_replica_providers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(replica_id): Path<i64>,
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
