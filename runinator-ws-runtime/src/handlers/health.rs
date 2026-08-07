use axum::{
    Extension, Json,
    http::{StatusCode, header},
    response::IntoResponse,
};
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::stability;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint};

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    status: String,
}

#[derive(Serialize, ToSchema)]
pub struct ReadinessResponse {
    status: String,
    database: String,
    broker_result_channels: bool,
    counters: stability::StabilityCounters,
}

/// liveness probe.
#[utoipa::path(
    get,
    path = "/health",
    tag = "Meta",
    security(),
    responses((status = 200, description = "service is up", body = HealthResponse)),
)]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
    })
}

/// prometheus metrics in the text exposition format.
#[utoipa::path(
    get,
    path = "/metrics",
    tag = "Meta",
    security(),
    responses((status = 200, description = "prometheus metrics", content_type = "text/plain")),
)]
pub async fn metrics() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        stability::render_metrics(),
    )
}

/// readiness probe: reports database and broker reachability.
#[utoipa::path(
    get,
    path = "/ready",
    tag = "Meta",
    security(),
    responses(
        (status = 200, description = "service is ready", body = ReadinessResponse),
        (status = 503, description = "a dependency is unavailable", body = ReadinessResponse),
    ),
)]
pub async fn ready<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(broker): Extension<Arc<dyn Broker>>,
) -> (StatusCode, Json<ReadinessResponse>) {
    // a cheap connectivity probe: fetch at most one row rather than the whole run history.
    let database_ready = db.fetch_recent_workflow_runs(1).await.is_ok();
    let status = if database_ready { "ready" } else { "not_ready" };
    let code = if database_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        code,
        Json(ReadinessResponse {
            status: status.into(),
            database: if database_ready { "ok" } else { "error" }.into(),
            broker_result_channels: broker.supports_workflow_result_channels(),
            counters: stability::snapshot(),
        }),
    )
}

/// the `health` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::get;
    axum::Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/ready", get(ready::<T>).layer(Extension(pool.clone())))
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/health",
        "Meta",
        "Check service health",
        "Returns a lightweight liveness response. This endpoint is public and does not touch the database.",
        true,
        None,
        &[],
        200,
        "service is alive",
        Example::Health,
    ),
    endpoint(
        "get",
        "/ready",
        "Meta",
        "Check service readiness",
        "Verifies that the web service can answer readiness checks, including database-dependent readiness.",
        true,
        None,
        &[],
        200,
        "service is ready",
        Example::Ready,
    ),
];
