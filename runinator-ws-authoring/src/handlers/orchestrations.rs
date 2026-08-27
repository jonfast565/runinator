use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
    routing::{get, post},
};
use chrono::Utc;
use runinator_models::{
    auth::{AuthContext, Permission},
    orchestration::{IngressEvent, IngressEventDisposition, OrchestrationStatus},
};
use runinator_store::roles::{DefinitionStore, IngressStore, OrchestrationStore};
use runinator_ws_core::{
    models::{ApiResponse, OrchestrationIntentRequest},
    responses::{api_error, bad_request, not_found},
};
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Default, Deserialize)]
pub struct OrchestrationQuery {
    pub status: Option<String>,
    pub pipeline_id: Option<Uuid>,
    pub scope: Option<String>,
    pub correlation_key: Option<String>,
    pub limit: Option<i64>,
}

async fn authorized_binding<T: AuthorizationStore + OrchestrationStore>(
    db: &T,
    ctx: &AuthContext,
    id: Uuid,
    permission: Permission,
) -> Result<runinator_models::orchestration::OrchestrationBinding, (StatusCode, Json<ApiResponse>)>
{
    let binding = db
        .fetch_orchestration_binding(id)
        .await
        .map_err(|error| api_error(error.to_string()))?
        .ok_or_else(|| not_found("orchestration not found"))?;
    AuthzChecker::new(db, ctx)
        .require_pipeline(binding.pipeline_id, permission)
        .await?;
    Ok(binding)
}

pub async fn list<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<OrchestrationQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let status = match query.status.as_deref() {
        None => None,
        Some("pending") => Some(OrchestrationStatus::Pending),
        Some("running") => Some(OrchestrationStatus::Running),
        Some("waiting") => Some(OrchestrationStatus::Waiting),
        Some("suspended") => Some(OrchestrationStatus::Suspended),
        Some("completed") => Some(OrchestrationStatus::Completed),
        Some("failed") => Some(OrchestrationStatus::Failed),
        Some("terminated") => Some(OrchestrationStatus::Terminated),
        Some(other) => return bad_request(format!("unknown orchestration status '{other}'")),
    };
    let bindings = match db
        .fetch_orchestration_bindings(ctx.org_id, status, query.limit.unwrap_or(200))
        .await
    {
        Ok(bindings) => bindings,
        Err(error) => return api_error(error.to_string()),
    };
    let visible = match AuthzChecker::new(db.as_ref(), &ctx)
        .visible_pipeline_ids()
        .await
    {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let bindings = bindings
        .into_iter()
        .filter(|binding| {
            visible
                .as_ref()
                .is_none_or(|ids| ids.contains(&binding.pipeline_id))
                && query.pipeline_id.is_none_or(|id| id == binding.pipeline_id)
                && query
                    .scope
                    .as_ref()
                    .is_none_or(|scope| scope == &binding.scope)
                && query
                    .correlation_key
                    .as_ref()
                    .is_none_or(|key| key == &binding.correlation_key)
        })
        .collect();
    (
        StatusCode::OK,
        Json(ApiResponse::OrchestrationBindingList(bindings)),
    )
}

pub async fn get_one<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match authorized_binding(db.as_ref(), &ctx, id, Permission::View).await {
        Ok(binding) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationBinding(binding)),
        ),
        Err(reply) => reply,
    }
}

pub async fn epochs<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorized_binding(db.as_ref(), &ctx, id, Permission::View).await {
        return reply;
    }
    match db.fetch_orchestration_epochs(id).await {
        Ok(values) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationEpochList(values)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn events<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorized_binding(db.as_ref(), &ctx, id, Permission::View).await {
        return reply;
    }
    match db.fetch_orchestration_reductions(id).await {
        Ok(values) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationReductionList(values)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn evidence<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorized_binding(db.as_ref(), &ctx, id, Permission::View).await {
        return reply;
    }
    match db.fetch_orchestration_evidence(id).await {
        Ok(values) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationEvidenceList(values)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn commands<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorized_binding(db.as_ref(), &ctx, id, Permission::View).await {
        return reply;
    }
    match db.fetch_orchestration_commands(id).await {
        Ok(values) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationCommandList(values)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn intent<T: AuthorizationStore + OrchestrationStore + IngressStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Json(request): Json<OrchestrationIntentRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let binding = match authorized_binding(db.as_ref(), &ctx, id, Permission::Run).await {
        Ok(binding) => binding,
        Err(reply) => return reply,
    };
    if request.reason.trim().is_empty() || request.idempotency_key.trim().is_empty() {
        return bad_request("reason and idempotency_key are required");
    }
    if !binding.policy.intents.contains_key(&request.intent) {
        return bad_request(format!("unknown orchestration intent '{}'", request.intent));
    }
    let event = IngressEvent {
        source: "runinator.manual".into(),
        event_id: request.idempotency_key,
        event_type: "manual_intent".into(),
        correlation_key: binding.correlation_key.clone(),
        payload: runinator_models::json!({
            "intent": request.intent, "payload": request.payload, "reason": request.reason,
            "actor_id": ctx.principal_id,
        }),
        occurred_at: Some(Utc::now()),
    };
    match db
        .record_ingress_event(
            binding.admission_id,
            binding.generation,
            event,
            IngressEventDisposition::Recorded,
            false,
            Utc::now(),
        )
        .await
    {
        Ok(record) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::IngressTimeline(vec![record.entry])),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub fn routes<T>(pool: Arc<T>) -> axum::Router
where
    T: AuthorizationStore + DefinitionStore + OrchestrationStore + IngressStore,
{
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_ORCHESTRATIONS,
            get(list::<T>),
        )
        .route("/orchestrations/{id}", get(get_one::<T>))
        .route("/orchestrations/{id}/events", get(events::<T>))
        .route("/orchestrations/{id}/epochs", get(epochs::<T>))
        .route("/orchestrations/{id}/evidence", get(evidence::<T>))
        .route("/orchestrations/{id}/commands", get(commands::<T>))
        .route("/orchestrations/{id}/intents", post(intent::<T>))
        .layer(Extension(pool))
}
