use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
    routing::{get, post},
};
use chrono::Utc;
use runinator_broker_core::{UiEventPublisher, emit_external_operation, emit_orchestration};
use runinator_engine::services::OrchestrationOperations;
use runinator_models::{
    auth::{AuthContext, Permission},
    orchestration::{
        DeliverySemantics, ExternalOperationStatus, IngressAdmissionStatus, IngressEvent,
        IngressEventDisposition, OrchestrationEvidence, OrchestrationStatus,
    },
    workflow_vm::WorkflowEffectStatus,
};
use runinator_store::roles::{
    DefinitionStore, ExternalOperationUpdate, IngressStore, OrchestrationStore, WorkflowVmStore,
    WorkspaceStore,
};
use runinator_ws_core::{
    models::{
        ApiResponse, ExternalOperationResolutionRequest, OrchestrationIntentRequest,
        OrchestrationRequeueRequest,
    },
    responses::{api_error, bad_request, not_found},
};
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Default, Deserialize)]
pub struct OrchestrationQuery {
    pub status: Option<String>,
    pub pipeline_id: Option<Uuid>,
    pub adapter_id: Option<Uuid>,
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
                    .adapter_id
                    .is_none_or(|id| binding.adapter_id == Some(id))
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

pub async fn operations<T: AuthorizationStore + OrchestrationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorized_binding(db.as_ref(), &ctx, id, Permission::View).await {
        return reply;
    }
    match db.fetch_external_operations(id).await {
        Ok(values) => (
            StatusCode::OK,
            Json(ApiResponse::ExternalOperationList(values)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn workspaces<T: AuthorizationStore + OrchestrationStore + WorkspaceStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let binding = match authorized_binding(db.as_ref(), &ctx, id, Permission::View).await {
        Ok(binding) => binding,
        Err(reply) => return reply,
    };
    match db
        .fetch_workspaces_for_admission(binding.admission_id, binding.generation)
        .await
    {
        Ok(values) => (StatusCode::OK, Json(ApiResponse::WorkspaceList(values))),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn resolve_operation<T: AuthorizationStore + OrchestrationStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(publisher): Extension<UiEventPublisher>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, operation_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ExternalOperationResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let binding = match authorized_binding(db.as_ref(), &ctx, id, Permission::Run).await {
        Ok(binding) => binding,
        Err(reply) => return reply,
    };
    if request.reason.trim().is_empty() {
        return bad_request("reason is required");
    }
    let operation = match db.fetch_external_operation(operation_id).await {
        Ok(Some(operation)) if operation.binding_id == id => operation,
        Ok(_) => return not_found("external operation not found"),
        Err(error) => return api_error(error.to_string()),
    };
    if !operation.ambiguous && operation.status != ExternalOperationStatus::Waiting {
        return bad_request("external operation is not waiting for ambiguity resolution");
    }
    if operation.epoch != binding.current_epoch {
        return bad_request("external operation belongs to a stale execution epoch");
    }
    let Some(effect_id) = operation.effect_id else {
        return bad_request("external operation has no durable effect receipt");
    };
    let attempt = match u32::try_from(operation.attempt) {
        Ok(attempt) => attempt,
        Err(_) => return bad_request("external operation attempt is invalid"),
    };
    let now = Utc::now();
    let status = match request.resolution.as_str() {
        "succeeded" => {
            let output = (!request.receipt.is_null()).then(|| request.receipt.clone());
            match db
                .settle_workflow_effect(
                    effect_id,
                    attempt,
                    WorkflowEffectStatus::Succeeded,
                    output,
                    Some(request.reason.clone()),
                    now,
                )
                .await
            {
                Ok(true) => ExternalOperationStatus::Succeeded,
                Ok(false) => return bad_request("external operation effect is no longer current"),
                Err(error) => return api_error(error.to_string()),
            }
        }
        "failed" => {
            match db
                .settle_workflow_effect(
                    effect_id,
                    attempt,
                    WorkflowEffectStatus::Failed,
                    None,
                    Some(request.reason.clone()),
                    now,
                )
                .await
            {
                Ok(true) => ExternalOperationStatus::Failed,
                Ok(false) => return bad_request("external operation effect is no longer current"),
                Err(error) => return api_error(error.to_string()),
            }
        }
        "retry" if operation.semantics != DeliverySemantics::AtLeastOnce => {
            match db
                .retry_workflow_effect(effect_id, attempt, now, Some(request.reason.clone()), now)
                .await
            {
                Ok(true) => ExternalOperationStatus::Pending,
                Ok(false) => return bad_request("external operation effect is no longer current"),
                Err(error) => return api_error(error.to_string()),
            }
        }
        "retry" => {
            return bad_request(
                "at-least-once operations cannot be retried after an ambiguous outcome",
            );
        }
        other => return bad_request(format!("unknown operation resolution '{other}'")),
    };
    let updated_attempt = if status == ExternalOperationStatus::Pending {
        operation.attempt + 1
    } else {
        operation.attempt
    };
    let updated = match db
        .update_external_operation(
            operation_id,
            ExternalOperationUpdate {
                status,
                attempt: updated_attempt,
                ambiguous: false,
                provenance: operation.provenance.clone(),
                receipt: request.receipt.clone(),
            },
            now,
        )
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return not_found("external operation not found"),
        Err(error) => return api_error(error.to_string()),
    };
    if let Err(error) = db
        .append_orchestration_evidence(OrchestrationEvidence {
            id: Uuid::now_v7(),
            binding_id: id,
            epoch: None,
            kind: "external_operation_resolution".into(),
            subject_revision: None,
            payload: runinator_models::json!({
                "operation_id": operation_id,
                "resolution": request.resolution,
                "reason": request.reason,
                "actor_id": ctx.principal_id,
            }),
            source_event_id: None,
            created_at: now,
        })
        .await
    {
        return api_error(error.to_string());
    }
    emit_external_operation(&publisher, updated.id, binding.id, binding.org_id);
    (
        StatusCode::OK,
        Json(ApiResponse::ExternalOperation(updated)),
    )
}

pub async fn intent<T: AuthorizationStore + OrchestrationStore + IngressStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(publisher): Extension<UiEventPublisher>,
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
        Ok(record) => {
            emit_orchestration(&publisher, binding.id, binding.org_id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::IngressTimeline(vec![record.entry])),
            )
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn requeue<
    T: AuthorizationStore + DefinitionStore + OrchestrationStore + IngressStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(publisher): Extension<UiEventPublisher>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Json(request): Json<OrchestrationRequeueRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let binding = match authorized_binding(db.as_ref(), &ctx, id, Permission::Run).await {
        Ok(binding) => binding,
        Err(reply) => return reply,
    };
    if !binding.status.is_terminal() {
        return bad_request("only a terminal orchestration can be requeued");
    }
    if request.reason.trim().is_empty() || request.idempotency_key.trim().is_empty() {
        return bad_request("reason and idempotency_key are required");
    }
    let admission = match db
        .fetch_ingress_admission(
            binding.org_id,
            binding.scope.clone(),
            binding.correlation_key.clone(),
        )
        .await
    {
        Ok(Some(admission)) if admission.status == IngressAdmissionStatus::Terminal => admission,
        Ok(Some(_)) => return bad_request("orchestration admission is not terminal"),
        Ok(None) => return not_found("orchestration admission not found"),
        Err(error) => return api_error(error.to_string()),
    };
    let Some(admission_id) = admission.id else {
        return api_error("stored orchestration admission has no id");
    };
    let event = IngressEvent {
        source: "runinator.manual_requeue".into(),
        event_id: request.idempotency_key,
        event_type: "manual_requeue".into(),
        correlation_key: binding.correlation_key.clone(),
        payload: runinator_models::json!({
            "reason": request.reason,
            "actor_id": ctx.principal_id,
            "previous_binding_id": binding.id,
        }),
        occurred_at: Some(Utc::now()),
    };
    let record = match db
        .requeue_ingress_event(
            admission_id,
            admission.generation,
            admission.target.clone(),
            admission.policy.clone(),
            event,
            Utc::now(),
        )
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) => return bad_request("another event already requeued this admission"),
        Err(error) => return api_error(error.to_string()),
    };
    let next_admission = match db
        .fetch_ingress_admission(
            binding.org_id,
            binding.scope.clone(),
            binding.correlation_key.clone(),
        )
        .await
    {
        Ok(Some(admission)) => admission,
        Ok(None) => return api_error("requeued admission disappeared"),
        Err(error) => return api_error(error.to_string()),
    };
    let pipeline = match db.fetch_pipelines().await {
        Ok(pipelines) => pipelines
            .into_iter()
            .find(|pipeline| pipeline.id == Some(binding.pipeline_id)),
        Err(error) => return api_error(error.to_string()),
    };
    let Some(pipeline) = pipeline else {
        return not_found("orchestration pipeline not found");
    };
    let adapter = match binding.adapter_id {
        Some(adapter_id) => match db.fetch_orchestration_adapter(adapter_id).await {
            Ok(Some(adapter)) => Some((adapter.id, adapter.current_revision)),
            Ok(None) => None,
            Err(error) => return api_error(error.to_string()),
        },
        None => None,
    };
    let operations = OrchestrationOperations::new(db.clone());
    match operations
        .admit_with_adapter(&next_admission, &pipeline, adapter)
        .await
    {
        Ok(Some(next)) => {
            emit_orchestration(&publisher, binding.id, binding.org_id);
            emit_orchestration(&publisher, next.id, next.org_id);
            (
                if record.duplicate {
                    StatusCode::OK
                } else {
                    StatusCode::ACCEPTED
                },
                Json(ApiResponse::OrchestrationBinding(next)),
            )
        }
        Ok(None) => bad_request("pipeline no longer has orchestration policy"),
        Err(error) => api_error(error.to_string()),
    }
}

pub fn routes<T>(pool: Arc<T>, publisher: UiEventPublisher) -> axum::Router
where
    T: AuthorizationStore
        + DefinitionStore
        + OrchestrationStore
        + IngressStore
        + WorkflowVmStore
        + WorkspaceStore,
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
        .route("/orchestrations/{id}/operations", get(operations::<T>))
        .route("/orchestrations/{id}/workspaces", get(workspaces::<T>))
        .route(
            "/orchestrations/{id}/operations/{operation_id}/resolve",
            post(resolve_operation::<T>),
        )
        .route("/orchestrations/{id}/intents", post(intent::<T>))
        .route("/orchestrations/{id}/requeue", post(requeue::<T>))
        .layer(Extension(pool))
        .layer(Extension(publisher))
}
