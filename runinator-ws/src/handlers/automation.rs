use std::{collections::HashMap, sync::Arc};

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::value::Value;

use crate::models::{
    ApiResponse, ApprovalResolutionRequest, AutomationRecordQuery, IdempotencyRequest,
};
use crate::repository;
use crate::responses::{api_error, not_found};

async fn list_records<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
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
        Ok(records) => (StatusCode::OK, Json(ApiResponse::JsonList(records))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn create_record<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    record_type: &'static str,
    Json(record): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_automation_record(db.as_ref(), record_type, record).await {
        Ok(record) => (StatusCode::ACCEPTED, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_external_items<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "external_items").await
}

pub(crate) async fn create_external_item<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "external_items", json).await
}

pub(crate) async fn get_external_resources<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "external_resources").await
}

pub(crate) async fn create_external_resource<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "external_resources", json).await
}

pub(crate) async fn get_feedback<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "feedback").await
}

pub(crate) async fn create_feedback<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "feedback", json).await
}

pub(crate) async fn get_gates<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "gates").await
}

pub(crate) async fn create_gate<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "gates", json).await
}

pub(crate) async fn get_workspaces<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "workspaces").await
}

pub(crate) async fn create_workspace<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "workspaces", json).await
}

pub(crate) async fn get_change_sets<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "change_sets").await
}

pub(crate) async fn create_change_set<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "change_sets", json).await
}

pub(crate) async fn get_automation_events<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "automation_events").await
}

pub(crate) async fn create_automation_event<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "automation_events", json).await
}

pub(crate) async fn get_approvals<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "approval_requests").await
}

pub(crate) async fn create_approval<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "approval_requests", json).await
}

pub(crate) async fn approve_request<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(approval_id): Path<i64>,
    Json(request): Json<ApprovalResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
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
    Path(approval_id): Path<i64>,
    Json(request): Json<ApprovalResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
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
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<ApiResponse>) {
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
    Json(request): Json<IdempotencyRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::put_idempotency_key(db.as_ref(), request.scope, request.key, request.result)
        .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}
