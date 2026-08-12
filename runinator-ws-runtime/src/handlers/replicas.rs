use std::{net::SocketAddr, sync::Arc};
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::{AuthContext, PrincipalKind},
    replicas::{
        ReplicaHeartbeatRequest, ReplicaOfflineRequest, ReplicaProviderRegistrationRequest,
        ReplicaRegistrationRequest,
    },
};

use crate::repository;
use runinator_ws_core::models::{ApiResponse, ReplicaQuery, ReplicaSampleQuery};
use runinator_ws_core::openapi::docs::{
    EndpointDoc, Example, REPLICA_FILTERS, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::AuthContextExt;

pub async fn register_replica<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Json(request): Json<ReplicaRegistrationRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_agent_service_or_admin() {
        return reply;
    }
    if matches!(ctx.kind, PrincipalKind::Agent) {
        match repository::fetch_replica_by_runtime(
            db.as_ref(),
            request.instance_id.clone(),
            request.runtime_id.clone(),
        )
        .await
        {
            Ok(Some(replica)) if replica.registered_by_principal_id != ctx.principal_id => {
                return not_found("Replica not found");
            }
            Err(err) => return api_error(err.to_string()),
            _ => {}
        }
    }
    match repository::register_replica(db.as_ref(), request, observed_ip(&headers, connect), &ctx)
        .await
    {
        Ok(replica) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn heartbeat_replica<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Path(replica_id): Path<Uuid>,
    Json(request): Json<ReplicaHeartbeatRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_agent_service_or_admin() {
        return reply;
    }
    if let Some(reply) = reject_unowned_agent_replica(db.as_ref(), &ctx, replica_id).await {
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

pub async fn mark_replica_offline<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    Json(request): Json<ReplicaOfflineRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_agent_service_or_admin() {
        return reply;
    }
    if let Some(reply) = reject_unowned_agent_replica(db.as_ref(), &ctx, replica_id).await {
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
pub async fn get_replicas<T: DatabaseImpl>(
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
pub async fn get_replica_samples<T: DatabaseImpl>(
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

pub async fn upsert_replica_provider<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    Json(request): Json<ReplicaProviderRegistrationRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_agent_service_or_admin() {
        return reply;
    }
    if let Some(reply) = reject_unowned_agent_replica(db.as_ref(), &ctx, replica_id).await {
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

pub async fn get_replica_providers<T: DatabaseImpl>(
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

async fn reject_unowned_agent_replica<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    replica_id: Uuid,
) -> Option<(StatusCode, Json<ApiResponse>)> {
    if !matches!(ctx.kind, PrincipalKind::Agent) {
        return None;
    }
    match repository::fetch_replica(db, replica_id).await {
        Ok(Some(replica)) if replica.registered_by_principal_id == ctx.principal_id => None,
        Ok(_) => Some(not_found("Replica not found")),
        Err(err) => Some(api_error(err.to_string())),
    }
}

/// the `replicas` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_REPLICAS,
            get(get_replicas::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/register",
            post(register_replica::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/{replica_id}/heartbeat",
            post(heartbeat_replica::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/{replica_id}/offline",
            post(mark_replica_offline::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/{replica_id}/providers",
            get(get_replica_providers::<T>)
                .post(upsert_replica_provider::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/{replica_id}/samples",
            get(get_replica_samples::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/replicas",
        "Replicas",
        "List service replicas",
        "Lists registered web, worker, waker, scheduler, and other runtime replicas, optionally filtered by kind or status.",
        false,
        None,
        REPLICA_FILTERS,
        200,
        "replicas",
        Example::ReplicaList,
    ),
    endpoint(
        "post",
        "/replicas/register",
        "Replicas",
        "Register a replica",
        "Registers a runtime replica and its advertised identity.",
        false,
        json_body("Replica registration record.", Example::Replica),
        &[],
        200,
        "registered replica",
        Example::Replica,
    ),
    endpoint(
        "post",
        "/replicas/{replica_id}/heartbeat",
        "Replicas",
        "Heartbeat a replica",
        "Updates a replica heartbeat and status so the service can track liveness.",
        false,
        json_body("Replica heartbeat fields.", Example::Replica),
        &[],
        200,
        "heartbeat recorded",
        Example::Replica,
    ),
    endpoint(
        "post",
        "/replicas/{replica_id}/offline",
        "Replicas",
        "Mark a replica offline",
        "Marks a registered replica offline.",
        false,
        None,
        &[],
        200,
        "replica marked offline",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/replicas/{replica_id}/providers",
        "Replicas",
        "List replica providers",
        "Lists provider registrations advertised by one replica.",
        false,
        None,
        &[],
        200,
        "replica providers",
        Example::ProviderList,
    ),
    endpoint(
        "post",
        "/replicas/{replica_id}/providers",
        "Replicas",
        "Upsert a replica provider",
        "Stores provider metadata advertised by one replica.",
        false,
        json_body("Replica provider registration.", Example::ReplicaProvider),
        &[],
        200,
        "replica provider stored",
        Example::ReplicaProvider,
    ),
];
