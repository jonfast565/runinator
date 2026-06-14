use axum::{Extension, Json, http::StatusCode};
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::stability;

#[derive(Serialize, ToSchema)]
pub(crate) struct HealthResponse {
    status: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ReadinessResponse {
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
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
    })
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
pub(crate) async fn ready<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(broker): Extension<Arc<dyn Broker>>,
) -> (StatusCode, Json<ReadinessResponse>) {
    let database_ready = db.fetch_recent_workflow_runs().await.is_ok();
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
