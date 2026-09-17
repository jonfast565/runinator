//! Read endpoints over the durable broker diagnostics and administrative audit trails.

use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode, response::IntoResponse};
use runinator_engine::services::DiagnosticsOperations;
use runinator_models::auth::AuthContext;
use runinator_models::diagnostics::{RuntimeLogBatch, RuntimeLogQuery};
use runinator_models::rbac::SystemRole;
use runinator_platform::env;
use runinator_store::{
    RuntimeStore,
    roles::{AutomationStore, DeliveryStore, WorkflowVmStore},
};

use runinator_ws_core::responses::{api_error, bad_request};
use runinator_ws_core::{
    models::{ApiResponse, AuditLogQuery, BrokerMessageQuery, DeadLetterQuery},
    validation::ValidatedJson,
};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker, IntoReply};

// cap the page size so a single query cannot scan an unbounded log.
const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 1000;
const DEFAULT_LOG_RETENTION_SECONDS: i64 = 24 * 60 * 60;

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
        return reply.into_reply();
    }
    match db
        .fetch_dead_letters(query.channel, clamp_limit(query.limit))
        .await
    {
        Ok(records) => (StatusCode::OK, Json(ApiResponse::JsonList(records))),
        Err(err) => api_error(err.to_string()),
    }
}

/// List engine-bound broker messages. An exact workflow run or pipeline run is authorized through
/// its owning resource; platform-wide inspection remains an operator-only capability.
#[utoipa::path(
    get,
    path = "/broker_messages",
    tag = "Observability",
    responses((status = 200, description = "broker message trace", body = [serde_json::Value])),
)]
pub async fn get_broker_messages<T: AuthorizationStore + DeliveryStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<BrokerMessageQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if [
        query.workflow_run_id,
        query.pipeline_run_id,
        query.adapter_id,
    ]
    .iter()
    .flatten()
    .count()
        > 1
    {
        return bad_request("select one workflow, pipeline run, or adapter");
    }
    let authorization = if let Some(adapter_id) = query.adapter_id {
        AuthzChecker::new(db.as_ref(), &ctx)
            .require_resource(
                runinator_models::auth::ResourceType::OrchestrationAdapter,
                adapter_id,
                runinator_models::auth::Permission::View,
            )
            .await
    } else if let Some(workflow_run_id) = query.workflow_run_id {
        AuthzChecker::new(db.as_ref(), &ctx)
            .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::View)
            .await
    } else if let Some(pipeline_run_id) = query.pipeline_run_id {
        AuthzChecker::new(db.as_ref(), &ctx)
            .require_pipeline_run(pipeline_run_id, runinator_models::auth::Permission::View)
            .await
    } else {
        ctx.require_scope_action(
            runinator_models::rbac::Action::DeadLettersRead,
            runinator_models::rbac::ScopeRef::PLATFORM,
        )
        .map_err(IntoReply::into_reply)
    };
    if let Err(reply) = authorization {
        return reply.into_reply();
    }
    match db
        .fetch_broker_messages(
            query.workflow_run_id,
            query.pipeline_run_id,
            query.adapter_id,
            query.channel,
            clamp_limit(query.limit),
        )
        .await
    {
        Ok(mut records) => {
            for record in &mut records {
                if record.adapter_id.is_some() {
                    let mut payload: serde_json::Value = record.payload.clone().into();
                    runinator_engine::services::redact_adapter_diagnostic(&mut payload);
                    record.payload = payload.into();
                }
            }
            (
                StatusCode::OK,
                Json(ApiResponse::BrokerMessageList(records)),
            )
        }
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
        return reply.into_reply();
    }
    match db
        .fetch_audit_log(query.actor_id, query.action, clamp_limit(query.limit))
        .await
    {
        Ok(records) => (StatusCode::OK, Json(ApiResponse::JsonList(records))),
        Err(err) => api_error(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/diagnostics/logs",
    tag = "Observability",
    request_body = serde_json::Value,
    responses((status = 202, description = "diagnostics batch accepted")),
)]
pub async fn post_runtime_logs<T: DeliveryStore + RuntimeStore + WorkflowVmStore>(
    Extension(operations): Extension<Arc<DiagnosticsOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(batch): ValidatedJson<RuntimeLogBatch>,
) -> axum::response::Response {
    if let Err(reply) = ctx.require_system_role(&[
        SystemRole::Engine,
        SystemRole::Worker,
        SystemRole::Waker,
        SystemRole::Agent,
        SystemRole::Replica,
    ]) {
        return reply.into_reply().into_response();
    }
    let mut records = Vec::with_capacity(batch.records.len());
    for mut record in batch.records {
        bind_runtime_log_identity(&mut record, ctx.system_role, ctx.principal_id);
        record.message.truncate(16 * 1024);
        records.push(record);
    }
    if let Err(error) = operations.record(records).await {
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::InvalidInput)
        {
            return bad_request(error.to_string()).into_response();
        }
        return api_error(error.to_string()).into_response();
    }
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "accepted": true })),
    )
        .into_response()
}

fn bind_runtime_log_identity(
    record: &mut runinator_models::diagnostics::RuntimeLogRecord,
    role: Option<SystemRole>,
    principal_id: Option<uuid::Uuid>,
) {
    let Some(role) = role else { return };
    record.source = role.as_str().to_string();
    record.runtime_id = principal_id.map(|id| id.to_string());
}

#[utoipa::path(
    get,
    path = "/diagnostics/logs",
    tag = "Observability",
    responses((status = 200, description = "runtime diagnostics", body = serde_json::Value)),
)]
pub async fn get_runtime_logs<T: AuthorizationStore + DeliveryStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<DiagnosticsOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<RuntimeLogQuery>,
) -> axum::response::Response {
    let correlated_run = if let Some(effect_id) = query.effect_id {
        match operations.run_for_effect(effect_id).await {
            Ok(Some(run_id)) => {
                if query.workflow_run_id.is_some_and(|id| id != run_id) {
                    return bad_request("the effect does not belong to the selected workflow run")
                        .into_response();
                }
                Some(run_id)
            }
            Ok(None) => {
                return bad_request("the selected workflow effect was not found").into_response();
            }
            Err(error) => return api_error(error.to_string()).into_response(),
        }
    } else {
        query.workflow_run_id
    };
    let authorization = if let Some(run_id) = correlated_run {
        AuthzChecker::new(db.as_ref(), &ctx)
            .require_run_workflow(run_id, runinator_models::auth::Permission::View)
            .await
    } else {
        ctx.require_scope_action(
            runinator_models::rbac::Action::DeadLettersRead,
            runinator_models::rbac::ScopeRef::PLATFORM,
        )
        .map_err(IntoReply::into_reply)
    };
    if let Err(reply) = authorization {
        return reply.into_reply().into_response();
    }
    let limit = clamp_limit(query.limit);
    match operations
        .fetch(&query, correlated_run, limit, log_retention_seconds())
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(error) => api_error(error.to_string()).into_response(),
    }
}

fn log_retention_seconds() -> i64 {
    env::parse_positive_or(
        "RUNINATOR_DIAGNOSTICS_RETENTION_SECONDS",
        DEFAULT_LOG_RETENTION_SECONDS,
    )
}

/// the `observability` endpoints.
pub fn routes<T: DeliveryStore + AutomationStore + AuthorizationStore + WorkflowVmStore>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::Extension;
    use axum::routing::get;
    let operations = Arc::new(DiagnosticsOperations::new(pool.clone()));
    axum::Router::new()
        .route(
            "/diagnostics/logs",
            get(get_runtime_logs::<T>)
                .post(post_runtime_logs::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/dead_letters",
            get(get_dead_letters::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/broker_messages",
            get(get_broker_messages::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/audit_log",
            get(get_audit_log::<T>).layer(Extension(pool.clone())),
        )
        .layer(Extension(operations))
}

#[cfg(test)]
#[path = "observability_tests.rs"]
mod tests;
