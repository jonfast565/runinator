//! admin-only read endpoints over the dead-letter queue and the audit log.

use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;

use crate::authz::require_admin;
use crate::models::{ApiResponse, AuditLogQuery, DeadLetterQuery};
use crate::responses::api_error;

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
pub(crate) async fn get_dead_letters<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<DeadLetterQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_admin(&ctx) {
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
pub(crate) async fn get_audit_log<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<AuditLogQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_admin(&ctx) {
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
