//! Resource-authorized adapter diagnostics and orchestration stepping.
use axum::{
    Extension, Json,
    extract::Path,
    http::StatusCode,
    routing::{get, post},
};
use runinator_engine::services::{
    AdapterOperations, OrchestrationOperations, redact_adapter_diagnostic,
};
use runinator_models::{
    adapter_control::{AdapterInspection, OrchestrationDebugControl},
    auth::{AuthContext, Permission, ResourceType},
    ingress_control::ExternalIngressGateMode,
};
use runinator_store::roles::OrchestrationStore;
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::{
    models::ApiResponse,
    openapi::docs::{EndpointDoc, Example, endpoint, json_body},
    responses::{api_error, bad_request, not_found},
};
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};
use std::sync::Arc;
use uuid::Uuid;

fn json(value: impl serde::Serialize) -> (StatusCode, Json<ApiResponse>) {
    match serde_json::to_value(value) {
        Ok(mut value) => {
            redact_adapter_diagnostic(&mut value);
            (StatusCode::OK, Json(ApiResponse::JsonValue(value.into())))
        }
        Err(error) => api_error(error.to_string()),
    }
}
async fn require_adapter<T: AuthorizationStore>(
    db: &T,
    ctx: &AuthContext,
    id: Uuid,
    permission: Permission,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    AuthzChecker::new(db, ctx)
        .require_resource(ResourceType::OrchestrationAdapter, id, permission)
        .await
}
pub async fn deliveries<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_adapter(db.as_ref(), &ctx, id, Permission::View).await {
        return reply;
    }
    match AdapterOperations::new(db).deliveries(id).await {
        Ok(value) => json(value),
        Err(error) => api_error(error.to_string()),
    }
}
pub async fn attempts<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_adapter(db.as_ref(), &ctx, id, Permission::View).await {
        return reply;
    }
    match AdapterOperations::new(db).attempts(id).await {
        Ok(value) => json(value),
        Err(error) => api_error(error.to_string()),
    }
}
pub async fn inspection<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_adapter(db.as_ref(), &ctx, id, Permission::View).await {
        return reply;
    }
    match AdapterOperations::new(db).inspection(id).await {
        Ok(value) => json(value),
        Err(error) => api_error(error.to_string()),
    }
}
#[derive(serde::Deserialize)]
pub struct InspectionRequest {
    mode: ExternalIngressGateMode,
}
pub async fn set_inspection<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<InspectionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_adapter(db.as_ref(), &ctx, id, Permission::Run).await {
        return reply;
    }
    match AdapterOperations::new(db)
        .set_inspection(AdapterInspection {
            adapter_id: id,
            mode: request.mode,
        })
        .await
    {
        Ok(()) => json(serde_json::json!({"mode":request.mode})),
        Err(error) => api_error(error.to_string()),
    }
}
pub async fn decide<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, delivery_id, decision)): Path<(Uuid, Uuid, String)>,
) -> (StatusCode, Json<ApiResponse>) {
    let service = AdapterOperations::new(db.clone());
    let record = match service.delivery(delivery_id).await {
        Ok(Some(record)) if record.origin.adapter_id == id => record,
        Ok(_) => return not_found("adapter delivery not found"),
        Err(error) => return api_error(error.to_string()),
    };
    if let Err(reply) =
        require_adapter(db.as_ref(), &ctx, record.origin.adapter_id, Permission::Run).await
    {
        return reply;
    }
    if !matches!(decision.as_str(), "approve" | "retry" | "drop") {
        return bad_request("decision must be approve, retry, or drop");
    }
    if record.event.is_none() && decision != "drop" {
        return bad_request("an unverified delivery cannot be replayed");
    }
    match service
        .decide_delivery(delivery_id, decision != "drop")
        .await
    {
        Ok(true) => json(serde_json::json!({"accepted":true})),
        Ok(false) => bad_request("delivery is not awaiting a decision"),
        Err(error) => api_error(error.to_string()),
    }
}
pub async fn debug_control<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(id, Permission::View)
        .await
    {
        return reply;
    }
    match OrchestrationOperations::new(db).debug_control(id).await {
        Ok(value) => json(value),
        Err(error) => api_error(error.to_string()),
    }
}
pub async fn set_debug_control<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(control): ValidatedJson<OrchestrationDebugControl>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(id, Permission::Run)
        .await
    {
        return reply;
    }
    if !(0..=1).contains(&control.steps) || (!control.paused && control.steps != 0) {
        return bad_request("steps must be zero or one; stepping requires pause");
    }
    match OrchestrationOperations::new(db)
        .set_debug_control(id, control.clone())
        .await
    {
        Ok(()) => json(control),
        Err(error) => api_error(error.to_string()),
    }
}
pub fn routes<T: AuthorizationStore + OrchestrationStore>(pool: Arc<T>) -> axum::Router {
    axum::Router::new()
        .route(
            "/orchestrations/adapters/{id}/deliveries",
            get(deliveries::<T>),
        )
        .route("/orchestrations/adapters/{id}/attempts", get(attempts::<T>))
        .route(
            "/orchestrations/adapters/{id}/inspection",
            get(inspection::<T>).put(set_inspection::<T>),
        )
        .route(
            "/orchestrations/adapters/{id}/deliveries/{delivery_id}/{decision}",
            post(decide::<T>),
        )
        .route(
            "/pipelines/{id}/orchestration-debug",
            get(debug_control::<T>).put(set_debug_control::<T>),
        )
        .layer(Extension(pool))
}

pub const DOCS: &[EndpointDoc] = &[
    endpoint!(
        "get",
        "/orchestrations/adapters/{id}/deliveries",
        "Adapter Control",
        "Inspect normalized adapter deliveries",
        "Inspect normalized adapter deliveries. Resource authorization is required.",
        false,
        None,
        &[],
        200,
        "Control result",
        Example::AdapterDeliveries
    ),
    endpoint!(
        "get",
        "/orchestrations/adapters/{id}/attempts",
        "Adapter Control",
        "Inspect adapter poll and test attempts",
        "Inspect adapter poll and test attempts. Resource authorization is required.",
        false,
        None,
        &[],
        200,
        "Control result",
        Example::AdapterAttempts
    ),
    endpoint!(
        "get",
        "/orchestrations/adapters/{id}/inspection",
        "Adapter Control",
        "Get adapter delivery gate",
        "Get adapter delivery gate. Resource authorization is required.",
        false,
        None,
        &[],
        200,
        "Control result",
        Example::AdapterInspection
    ),
    endpoint!(
        "put",
        "/orchestrations/adapters/{id}/inspection",
        "Adapter Control",
        "Set adapter delivery gate",
        "Set adapter delivery gate. Resource authorization is required.",
        false,
        json_body("Control decision", Example::AdapterInspection),
        &[],
        200,
        "Control result",
        Example::AdapterInspection
    ),
    endpoint!(
        "post",
        "/orchestrations/adapters/{id}/deliveries/{delivery_id}/{decision}",
        "Adapter Control",
        "Approve retry or drop a delivery",
        "Approve retry or drop a delivery. Resource authorization is required.",
        false,
        None,
        &[],
        200,
        "Control result",
        Example::AdapterDecision
    ),
    endpoint!(
        "get",
        "/pipelines/{id}/orchestration-debug",
        "Adapter Control",
        "Get orchestration reducer gate",
        "Get orchestration reducer gate. Resource authorization is required.",
        false,
        None,
        &[],
        200,
        "Control result",
        Example::OrchestrationDebugControl
    ),
    endpoint!(
        "put",
        "/pipelines/{id}/orchestration-debug",
        "Adapter Control",
        "Pause resume or step orchestration",
        "Pause resume or step orchestration. Resource authorization is required.",
        false,
        json_body("Control decision", Example::OrchestrationDebugControl),
        &[],
        200,
        "Control result",
        Example::OrchestrationDebugControl
    ),
];

impl runinator_models::validation::Validate for InspectionRequest {
    fn validate(&self) -> Result<(), runinator_models::validation::ValidationError> {
        // the enum has no unconstrained fields.
        Ok(())
    }
}
