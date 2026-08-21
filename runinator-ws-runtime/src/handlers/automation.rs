use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::{AuthContext, Permission};
use runinator_models::orchestration::{
    IdempotencyClaimRequest, IdempotencyCompleteRequest, IdempotencyReleaseRequest,
};
use runinator_models::value::Value;
use runinator_models::web::TaskResponse;

use crate::repository;
use runinator_ws_core::models::{
    ApiResponse, AutomationRecordQuery, GateQuery, IdempotencyRequest,
};
use runinator_ws_core::openapi::docs::{
    AUTOMATION_FILTERS, EndpointDoc, Example, GATE_FILTERS, IDEMPOTENCY_QUERY, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::AuthContextExt;
use runinator_ws_middleware::authz::AuthzChecker;

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
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::create_automation_record(db.as_ref(), record_type, record).await {
        Ok(record) => (StatusCode::ACCEPTED, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_external_items<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, &ctx, query, "external_items").await
}

pub async fn create_external_item<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, &ctx, "external_items", json).await
}

pub async fn get_gates<T: DatabaseImpl>(
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

pub async fn get_gate<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(gate_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_gate_workflow(gate_id, Permission::View)
        .await
    {
        return reply;
    }
    match repository::fetch_gate(db.as_ref(), gate_id).await {
        Ok(Some(record)) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Ok(None) => not_found("Gate not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_gate<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(record): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::create_gate(db.as_ref(), record).await {
        Ok(record) => (StatusCode::ACCEPTED, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_gate<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(gate_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_gate_workflow(gate_id, Permission::Edit)
        .await
    {
        return reply;
    }
    match repository::delete_gate(db.as_ref(), gate_id).await {
        Ok(true) => (StatusCode::OK, Json(ApiResponse::JsonValue(Value::Null))),
        Ok(false) => not_found("Gate not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_automation_events<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, &ctx, query, "automation_events").await
}

pub async fn create_automation_event<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, &ctx, "automation_events", json).await
}

pub async fn delete_automation_event<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(event_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_automation_record_workflow("automation_events", event_id, Permission::Edit)
        .await
    {
        return reply;
    }
    match repository::delete_automation_record(db.as_ref(), "automation_events", event_id).await {
        Ok(true) => (StatusCode::OK, Json(ApiResponse::JsonValue(Value::Null))),
        Ok(false) => not_found("Automation event not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_approvals<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, &ctx, query, "approval_requests").await
}

pub async fn create_approval<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    json: Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, &ctx, "approval_requests", json).await
}

pub async fn get_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
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

pub async fn put_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<IdempotencyRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::put_idempotency_key(db.as_ref(), request.scope, request.key, request.result)
        .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn claim_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<IdempotencyClaimRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::claim_idempotency_key(
        db.as_ref(),
        request.scope,
        request.key,
        request.owner_node_run_id,
        request.lease_seconds,
    )
    .await
    {
        Ok(claim) => match Value::encode(&claim) {
            Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value))),
            Err(err) => api_error(err.to_string()),
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn complete_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<IdempotencyCompleteRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::complete_idempotency_key(
        db.as_ref(),
        request.scope,
        request.key,
        request.owner_node_run_id,
        request.result,
    )
    .await
    {
        Ok(recorded) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: recorded,
                message: if recorded {
                    "Idempotency key completed".into()
                } else {
                    "Idempotency key not owned by this node run, or already completed".into()
                },
            })),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn release_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<IdempotencyReleaseRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::release_idempotency_key(
        db.as_ref(),
        request.scope,
        request.key,
        request.owner_node_run_id,
    )
    .await
    {
        Ok(released) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: released,
                message: if released {
                    "Idempotency reservation released".into()
                } else {
                    "Idempotency key not held by this node run, or already completed".into()
                },
            })),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn filter_records<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    records: Vec<Value>,
) -> Result<Vec<Value>, (StatusCode, Json<ApiResponse>)> {
    if ctx.is_platform_admin()
        || ctx
            .require_system_role(&[
                runinator_models::rbac::SystemRole::Engine,
                runinator_models::rbac::SystemRole::Worker,
            ])
            .is_ok()
    {
        return Ok(records);
    }
    let Some(visible) = AuthzChecker::new(db, ctx).visible_workflow_ids().await? else {
        return Ok(records);
    };
    let mut filtered = Vec::with_capacity(records.len());
    for record in records {
        let Some(workflow_run_id) = runinator_ws_middleware::authz::record_workflow_run_id(&record)
        else {
            continue;
        };
        let Some(run) = repository::fetch_workflow_run(db, workflow_run_id)
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

/// the `automation` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route(
            "/external_items",
            get(get_external_items::<T>)
                .post(create_external_item::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/gates",
            get(get_gates::<T>)
                .post(create_gate::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/gates/{id}",
            get(get_gate::<T>)
                .delete(delete_gate::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/automation_events",
            get(get_automation_events::<T>)
                .post(create_automation_event::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/automation_events/{id}",
            delete(delete_automation_event::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/approvals",
            get(get_approvals::<T>)
                .post(create_approval::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/idempotency_keys",
            get(get_idempotency_key::<T>)
                .post(put_idempotency_key::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/idempotency_keys/claim",
            post(claim_idempotency_key::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/idempotency_keys/complete",
            post(complete_idempotency_key::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/idempotency_keys/release",
            post(release_idempotency_key::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/external_items",
        "Automation",
        "List external items",
        "Lists external automation records, optionally filtered by workflow run or linked item.",
        false,
        None,
        AUTOMATION_FILTERS,
        200,
        "external items",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/external_items",
        "Automation",
        "Create an external item",
        "Creates an external automation record. Service credentials or admin privileges are required.",
        false,
        json_body("External item record.", Example::AutomationRecord),
        &[],
        202,
        "external item created",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/gates",
        "Automation",
        "List gates",
        "Lists gate records, optionally filtered by workflow run or status.",
        false,
        None,
        GATE_FILTERS,
        200,
        "gates",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/gates",
        "Automation",
        "Create a gate",
        "Creates a gate automation record. Service credentials or admin privileges are required.",
        false,
        json_body("Gate record.", Example::AutomationRecord),
        &[],
        202,
        "gate created",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/gates/{id}",
        "Automation",
        "Get a gate",
        "Fetches one gate record by id if the caller can view the owning workflow.",
        false,
        None,
        &[],
        200,
        "gate",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/automation_events",
        "Automation",
        "List automation events",
        "Lists generic automation event records.",
        false,
        None,
        AUTOMATION_FILTERS,
        200,
        "automation events",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/automation_events",
        "Automation",
        "Create an automation event",
        "Creates a generic automation event record. Service credentials or admin privileges are required.",
        false,
        json_body("Automation event record.", Example::AutomationRecord),
        &[],
        202,
        "automation event created",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/approvals",
        "Automation",
        "List approval requests",
        "Lists approval request records, optionally filtered by workflow run or linked item.",
        false,
        None,
        AUTOMATION_FILTERS,
        200,
        "approval requests",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/approvals",
        "Automation",
        "Create an approval request",
        "Creates an approval request record. Service credentials or admin privileges are required.",
        false,
        json_body("Approval request record.", Example::AutomationRecord),
        &[],
        202,
        "approval request created",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/idempotency_keys",
        "Control Plane",
        "Get an idempotency key",
        "Fetches a stored idempotency result by scope and key. Service credentials or admin privileges are required.",
        false,
        None,
        IDEMPOTENCY_QUERY,
        200,
        "idempotency result",
        Example::Idempotency,
    ),
    endpoint(
        "post",
        "/idempotency_keys",
        "Control Plane",
        "Put an idempotency key",
        "Stores an idempotency result for later duplicate-request suppression.",
        false,
        json_body("Idempotency scope, key, and result.", Example::Idempotency),
        &[],
        200,
        "idempotency key stored",
        Example::Idempotency,
    ),
];
