use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::{AuthContext, Permission, PrincipalKind};
use runinator_models::value::Value;

use crate::models::{
    ApiResponse, ApprovalResolutionRequest, AutomationRecordQuery, GateQuery,
    GateResolutionRequest, IdempotencyRequest,
};
use crate::repository;
use crate::responses::{api_error, not_found};

async fn list_records<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    ctx: &AuthContext,
    Query(query): Query<AutomationRecordQuery>,
    record_type: &'static str,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_automation_records(
        db.as_ref(),
        record_type,
        query.workflow_run_id,
        query.external_item_id,
    )
    .await
    {
        Ok(records) => match filter_records(db.as_ref(), ctx, records).await {
            Ok(records) => (StatusCode::OK, Json(ApiResponse::JsonList(records))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

async fn create_record<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    ctx: &AuthContext,
    record_type: &'static str,
    Json(record): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(ctx) {
        return reply;
    }
    match repository::create_automation_record(db.as_ref(), record_type, record).await {
        Ok(record) => (StatusCode::ACCEPTED, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_external_items<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, &ctx, query, "external_items").await
}

pub(crate) async fn create_external_item<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, &ctx, "external_items", json).await
}

pub(crate) async fn get_gates<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<GateQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_gates(db.as_ref(), query.workflow_run_id, query.status).await {
        Ok(records) => match filter_records(db.as_ref(), &ctx, records).await {
            Ok(records) => (StatusCode::OK, Json(ApiResponse::JsonList(records))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_gate<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(gate_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_gate_workflow(db.as_ref(), &ctx, gate_id, Permission::View).await
    {
        return reply;
    }
    match repository::fetch_gate(db.as_ref(), gate_id).await {
        Ok(Some(record)) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Ok(None) => not_found("Gate not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_gate<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(record): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::create_gate(db.as_ref(), record).await {
        Ok(record) => (StatusCode::ACCEPTED, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/gates/{id}/open",
    tag = "Automation",
    params(("id" = Uuid, Path, description = "Gate identifier.")),
    request_body = crate::models::GateResolutionRequest,
    responses(
        (status = 200, description = "gate opened", body = serde_json::Value),
        (status = 401, description = "request is missing or has an invalid credential", body = crate::models::ApiError),
        (status = 404, description = "gate was not found", body = crate::models::ApiError),
    ),
)]
pub(crate) async fn open_gate<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(gate_id): Path<Uuid>,
    Json(request): Json<GateResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_gate_workflow(db.as_ref(), &ctx, gate_id, Permission::Run).await
    {
        return reply;
    }
    match repository::resolve_gate(
        db.as_ref(),
        gate_id,
        true,
        request.reason,
        request.resolved_by,
    )
    .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/gates/{id}/close",
    tag = "Automation",
    params(("id" = Uuid, Path, description = "Gate identifier.")),
    request_body = crate::models::GateResolutionRequest,
    responses(
        (status = 200, description = "gate closed", body = serde_json::Value),
        (status = 401, description = "request is missing or has an invalid credential", body = crate::models::ApiError),
        (status = 404, description = "gate was not found", body = crate::models::ApiError),
    ),
)]
pub(crate) async fn close_gate<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(gate_id): Path<Uuid>,
    Json(request): Json<GateResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        crate::authz::require_gate_workflow(db.as_ref(), &ctx, gate_id, Permission::Run).await
    {
        return reply;
    }
    match repository::resolve_gate(
        db.as_ref(),
        gate_id,
        false,
        request.reason,
        request.resolved_by,
    )
    .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_automation_events<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, &ctx, query, "automation_events").await
}

pub(crate) async fn create_automation_event<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, &ctx, "automation_events", json).await
}

pub(crate) async fn get_approvals<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, &ctx, query, "approval_requests").await
}

pub(crate) async fn create_approval<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, &ctx, "approval_requests", json).await
}

pub(crate) async fn approve_request<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(approval_id): Path<Uuid>,
    Json(request): Json<ApprovalResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_automation_record_workflow(
        db.as_ref(),
        &ctx,
        "approval_requests",
        approval_id,
        Permission::Run,
    )
    .await
    {
        return reply;
    }
    match repository::resolve_approval(
        db.as_ref(),
        approval_id,
        true,
        request.resolved_by,
        request.message,
        request.output_json,
    )
    .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn reject_request<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(approval_id): Path<Uuid>,
    Json(request): Json<ApprovalResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_automation_record_workflow(
        db.as_ref(),
        &ctx,
        "approval_requests",
        approval_id,
        Permission::Run,
    )
    .await
    {
        return reply;
    }
    match repository::resolve_approval(
        db.as_ref(),
        approval_id,
        false,
        request.resolved_by,
        request.message,
        request.output_json,
    )
    .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    let Some(scope) = query.get("scope").cloned() else {
        return api_error("idempotency query requires scope");
    };
    let Some(key) = query.get("key").cloned() else {
        return api_error("idempotency query requires key");
    };
    match repository::fetch_idempotency_key(db.as_ref(), scope, key).await {
        Ok(Some(record)) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Ok(None) => not_found("idempotency key not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn put_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<IdempotencyRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::put_idempotency_key(db.as_ref(), request.scope, request.key, request.result)
        .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn filter_records<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    records: Vec<Value>,
) -> Result<Vec<Value>, (StatusCode, Json<ApiResponse>)> {
    if ctx.is_admin || matches!(ctx.kind, PrincipalKind::Service) {
        return Ok(records);
    }
    let Some(visible) = crate::authz::visible_workflow_ids(db, ctx).await else {
        return Ok(records);
    };
    let mut filtered = Vec::with_capacity(records.len());
    for record in records {
        let Some(workflow_run_id) = crate::authz::record_workflow_run_id(&record) else {
            continue;
        };
        let Some((run, _)) = repository::fetch_workflow_run(db, workflow_run_id)
            .await
            .map_err(|err| api_error(err.to_string()))?
        else {
            continue;
        };
        if visible.contains(&run.workflow_id) {
            filtered.push(record);
        }
    }
    Ok(filtered)
}
