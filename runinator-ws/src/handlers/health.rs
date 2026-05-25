use axum::{Extension, Json, http::StatusCode};
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use serde::Serialize;
use std::sync::Arc;

use crate::stability;

#[derive(Serialize)]
pub(crate) struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
pub(crate) struct ReadinessResponse {
    status: &'static str,
    database: &'static str,
    broker_result_channels: bool,
    counters: stability::StabilityCounters,
}

pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

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
            status,
            database: if database_ready { "ok" } else { "error" },
            broker_result_channels: broker.supports_workflow_result_channels(),
            counters: stability::snapshot(),
        }),
    )
}
