use std::{net::SocketAddr, sync::Arc};
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_engine::services::ReplicaRegistry;
use runinator_models::{
    auth::AuthContext,
    replicas::{
        ReplicaHeartbeatRequest, ReplicaOfflineRequest, ReplicaProviderRegistrationRequest,
        ReplicaRegistrationRequest,
    },
};
use runinator_store::{RuntimeStore, roles::ReplicaStore};

use runinator_ws_core::ValidatedJson;
use runinator_ws_core::events::{AppEvent, AppEventKind, EventSender, emit};
use runinator_ws_core::models::{ApiResponse, ReplicaQuery, ReplicaSampleQuery};
use runinator_ws_core::openapi::docs::{
    EndpointDoc, Example, REPLICA_FILTERS, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::AuthContextExt;

pub async fn register_replica<T: ReplicaStore>(
    Extension(registry): Extension<Arc<ReplicaRegistry<T>>>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    ValidatedJson(request): ValidatedJson<ReplicaRegistrationRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Agent,
        runinator_models::rbac::SystemRole::Replica,
    ]) {
        return reply;
    }
    match registry
        .agent_owns_runtime_registration(&ctx, &request)
        .await
    {
        Ok(true) => {}
        Ok(false) => return not_found("Replica not found"),
        Err(err) => return api_error(err.to_string()),
    }
    match registry
        .register(request, observed_ip(&headers, connect), &ctx)
        .await
    {
        Ok(replica) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn heartbeat_replica<T: ReplicaStore>(
    Extension(registry): Extension<Arc<ReplicaRegistry<T>>>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Path(replica_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<ReplicaHeartbeatRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Agent,
        runinator_models::rbac::SystemRole::Replica,
    ]) {
        return reply;
    }
    if let Some(reply) = reject_unowned_agent_replica(&registry, &ctx, replica_id).await {
        return reply;
    }
    match registry
        .heartbeat(replica_id, request, observed_ip(&headers, connect))
        .await
    {
        Ok(Some(replica)) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Ok(None) => not_found(format!(
            "Replica {replica_id} not found or runtime mismatch"
        )),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn mark_replica_offline<T: ReplicaStore>(
    Extension(registry): Extension<Arc<ReplicaRegistry<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<ReplicaOfflineRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Agent,
        runinator_models::rbac::SystemRole::Replica,
    ]) {
        return reply;
    }
    if let Some(reply) = reject_unowned_agent_replica(&registry, &ctx, replica_id).await {
        return reply;
    }
    match registry.mark_offline(replica_id, request.runtime_id).await {
        Ok(Some(replica)) => (StatusCode::OK, Json(ApiResponse::Replica(replica))),
        Ok(None) => not_found(format!(
            "Replica {replica_id} not found or runtime mismatch"
        )),
        Err(err) => api_error(err.to_string()),
    }
}

/// End one activation while leaving the enrolled machine credential available for a future fresh
/// activation.
pub async fn kick_replica<T: ReplicaStore>(
    Extension(registry): Extension<Arc<ReplicaRegistry<T>>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::NodesOperate,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    if let Some(org_id) = ctx.org_id {
        match registry.fetch(replica_id).await {
            Ok(Some(replica)) if replica.registered_by_org_id == Some(org_id) => {}
            Ok(_) => return not_found("Replica not found"),
            Err(err) => return api_error(err.to_string()),
        }
    }
    match registry.kick(replica_id).await {
        Ok(Some(replica)) => {
            emit(
                &events,
                AppEvent::new(replica.registered_by_org_id, AppEventKind::ReplicasChanged),
            );
            (StatusCode::OK, Json(ApiResponse::Replica(replica)))
        }
        Ok(None) => not_found(format!("Replica {replica_id} not found")),
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
pub async fn get_replicas<T: ReplicaStore + RuntimeStore>(
    Extension(registry): Extension<Arc<ReplicaRegistry<T>>>,
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<ReplicaQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply;
    }
    let settings = runinator_engine::settings::load_server_settings(db.as_ref())
        .await
        .unwrap_or_default();
    match registry
        .list_with_stale_after(
            query.replica_type,
            query.status,
            settings.replicas.stale_after_seconds as i64,
        )
        .await
    {
        Ok(mut replicas) => {
            if let Some(org_id) = ctx.org_id {
                replicas
                    .replicas
                    .retain(|replica| replica.registered_by_org_id == Some(org_id));
                replicas.running_tasks.retain(|id, _| {
                    replicas
                        .replicas
                        .iter()
                        .any(|replica| replica.replica_id == *id)
                });
                replicas.counts.workers = replicas
                    .replicas
                    .iter()
                    .filter(|r| {
                        r.replica_type == runinator_models::replicas::ReplicaKind::Worker
                            && r.status == runinator_models::replicas::ReplicaStatus::Live
                    })
                    .count() as i64;
                replicas.counts.wakers = replicas
                    .replicas
                    .iter()
                    .filter(|r| {
                        r.replica_type == runinator_models::replicas::ReplicaKind::Waker
                            && r.status == runinator_models::replicas::ReplicaStatus::Live
                    })
                    .count() as i64;
                replicas.counts.webservices = replicas
                    .replicas
                    .iter()
                    .filter(|r| {
                        r.replica_type == runinator_models::replicas::ReplicaKind::Webservice
                            && r.status == runinator_models::replicas::ReplicaStatus::Live
                    })
                    .count() as i64;
                replicas.counts.background = replicas
                    .replicas
                    .iter()
                    .filter(|r| {
                        r.replica_type == runinator_models::replicas::ReplicaKind::Background
                            && r.status == runinator_models::replicas::ReplicaStatus::Live
                    })
                    .count() as i64;
            }
            (StatusCode::OK, Json(ApiResponse::ReplicaList(replicas)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// fetch a replica's recent telemetry samples for charting.
pub async fn get_replica_samples<T: ReplicaStore + RuntimeStore>(
    Extension(registry): Extension<Arc<ReplicaRegistry<T>>>,
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    Query(query): Query<ReplicaSampleQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply;
    }
    if let Some(org_id) = ctx.org_id {
        match registry.fetch(replica_id).await {
            Ok(Some(replica)) if replica.registered_by_org_id == Some(org_id) => {}
            Ok(_) => return not_found("Replica not found"),
            Err(err) => return api_error(err.to_string()),
        }
    }
    let settings = runinator_engine::settings::load_server_settings(db.as_ref())
        .await
        .unwrap_or_default();
    match registry
        .samples_with_limits(
            replica_id,
            query.since_seconds,
            settings.replicas.sample_window_seconds as i64,
            settings.replicas.sample_max_points as i64,
        )
        .await
    {
        Ok(series) => (StatusCode::OK, Json(ApiResponse::ReplicaSamples(series))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn upsert_replica_provider<T: ReplicaStore>(
    Extension(registry): Extension<Arc<ReplicaRegistry<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<ReplicaProviderRegistrationRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Agent,
        runinator_models::rbac::SystemRole::Replica,
    ]) {
        return reply;
    }
    if let Some(reply) = reject_unowned_agent_replica(&registry, &ctx, replica_id).await {
        return reply;
    }
    match registry.upsert_provider(replica_id, request).await {
        Ok(registration) => (
            StatusCode::OK,
            Json(ApiResponse::ReplicaProviderRegistration(registration)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_replica_providers<T: ReplicaStore>(
    Extension(registry): Extension<Arc<ReplicaRegistry<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, ctx.selected_scope())
    {
        return reply;
    }
    if let Some(org_id) = ctx.org_id {
        match registry.fetch(replica_id).await {
            Ok(Some(replica)) if replica.registered_by_org_id == Some(org_id) => {}
            Ok(_) => return not_found("Replica not found"),
            Err(err) => return api_error(err.to_string()),
        }
    }
    match registry.providers(replica_id).await {
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

async fn reject_unowned_agent_replica<T: ReplicaStore>(
    registry: &ReplicaRegistry<T>,
    ctx: &AuthContext,
    replica_id: Uuid,
) -> Option<(StatusCode, Json<ApiResponse>)> {
    match registry.agent_owns_replica(ctx, replica_id).await {
        Ok(true) => None,
        Ok(false) => Some(not_found("Replica not found")),
        Err(err) => Some(api_error(err.to_string())),
    }
}

/// the `replicas` endpoints.
pub fn routes<T: ReplicaStore + RuntimeStore>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    let registry = Arc::new(ReplicaRegistry::new(pool.clone()));
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_REPLICAS,
            get(get_replicas::<T>).layer(Extension(registry.clone())),
        )
        .route(
            "/replicas/register",
            post(register_replica::<T>).layer(Extension(registry.clone())),
        )
        .route(
            "/replicas/{replica_id}/heartbeat",
            post(heartbeat_replica::<T>).layer(Extension(registry.clone())),
        )
        .route(
            "/replicas/{replica_id}/offline",
            post(mark_replica_offline::<T>).layer(Extension(registry.clone())),
        )
        .route(
            "/replicas/{replica_id}/kick",
            post(kick_replica::<T>).layer(Extension(registry.clone())),
        )
        .route(
            "/replicas/{replica_id}/providers",
            get(get_replica_providers::<T>)
                .post(upsert_replica_provider::<T>)
                .layer(Extension(registry.clone())),
        )
        .route(
            "/replicas/{replica_id}/samples",
            get(get_replica_samples::<T>).layer(Extension(registry)),
        )
        .layer(Extension(pool))
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint!(
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
    endpoint!(
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
    endpoint!(
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
    endpoint!(
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
    endpoint!(
        "post",
        "/replicas/{replica_id}/kick",
        "Replicas",
        "Kick a replica",
        "Ends one runtime activation and prevents that replica id from heartbeating or re-registering. The machine enrollment remains valid.",
        false,
        None,
        &[],
        200,
        "replica kicked",
        Example::Replica,
    ),
    endpoint!(
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
    endpoint!(
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
