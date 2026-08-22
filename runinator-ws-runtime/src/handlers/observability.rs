//! admin-only read endpoints over the dead-letter queue and the audit log.

use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_models::auth::AuthContext;
use runinator_store::roles::{AutomationStore, DeliveryStore};

use runinator_ws_core::models::{ApiResponse, AuditLogQuery, DeadLetterQuery};
use runinator_ws_core::responses::api_error;
use runinator_ws_middleware::authz::AuthContextExt;

// cap the page size so a single query cannot scan an unbounded log.
const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 1000;

fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// list dead-lettered broker messages, newest first.
#[utoipa::path(
    get,
    path = "/dead_letters",
    tag = "Observability",
    responses((status = 200, description = "dead-lettered messages", body = [serde_json::Value])),
)]
pub async fn get_dead_letters<T: DeliveryStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<DeadLetterQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::DeadLettersRead,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    match db
        .fetch_dead_letters(query.channel, clamp_limit(query.limit))
        .await
    {
        Ok(records) => (StatusCode::OK, Json(ApiResponse::JsonList(records))),
        Err(err) => api_error(err.to_string()),
    }
}

/// list audit-log entries, newest first.
#[utoipa::path(
    get,
    path = "/audit_log",
    tag = "Observability",
    responses((status = 200, description = "audit-log entries", body = [serde_json::Value])),
)]
pub async fn get_audit_log<T: AutomationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<AuditLogQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::AuditRead,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    match db
        .fetch_audit_log(query.actor_id, query.action, clamp_limit(query.limit))
        .await
    {
        Ok(records) => (StatusCode::OK, Json(ApiResponse::JsonList(records))),
        Err(err) => api_error(err.to_string()),
    }
}

/// the `observability` endpoints.
pub fn routes<T: DeliveryStore + AutomationStore>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::get;
    axum::Router::new()
        .route(
            "/dead_letters",
            get(get_dead_letters::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/audit_log",
            get(get_audit_log::<T>).layer(Extension(pool.clone())),
        )
}
