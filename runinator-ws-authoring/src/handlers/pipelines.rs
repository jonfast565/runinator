use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_models::{
    auth::{AuthContext, Permission},
    pipelines::{Pipeline, PipelineTrigger},
};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, IngressStore, OrchestrationStore, RbacStore, ScheduleStore,
        WorkflowVmStore,
    },
};
use serde::Deserialize;

use runinator_engine::services::{
    OrchestrationOperations, PipelineIngressError, PipelineIngressRequest, PipelineOperations,
};
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::models::{
    ApiError, ApiResponse, IngressEventRequest, IngressResponse, ManagedRunOverrideRequest,
    PipelineMemberRetryRequest, PipelineRunInquiryDecision, PipelineRunRequest,
    PipelineRunResolutionRequest,
};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker};

pub async fn get_pipelines<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    match service.list().await {
        Ok(pipelines) => {
            let visible = AuthzChecker::new(db.as_ref(), &ctx)
                .visible_pipeline_ids()
                .await;
            let pipelines = match visible {
                Ok(Some(ids)) => pipelines
                    .into_iter()
                    .filter(|pipeline| pipeline.id.is_some_and(|id| ids.contains(&id)))
                    .collect(),
                Ok(None) => pipelines,
                Err(reply) => return reply,
            };
            (StatusCode::OK, Json(ApiResponse::PipelineList(pipelines)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_pipeline<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::View)
        .await
    {
        return reply;
    }
    match service.fetch(pipeline_id).await {
        Ok(Some(pipeline)) => (StatusCode::OK, Json(ApiResponse::Pipeline(pipeline))),
        Ok(None) => not_found(format!("Pipeline {pipeline_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

const DEFAULT_REVISION_LIMIT: i64 = 50;
const MAX_REVISION_LIMIT: i64 = 500;

#[derive(Debug, Deserialize)]
pub struct PipelineRevisionListQuery {
    limit: Option<i64>,
}

pub async fn get_pipeline_revisions<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    Query(query): Query<PipelineRevisionListQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::View)
        .await
    {
        return reply;
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_REVISION_LIMIT)
        .clamp(1, MAX_REVISION_LIMIT);
    match service.revisions(pipeline_id, limit).await {
        Ok(revisions) => (
            StatusCode::OK,
            Json(ApiResponse::PipelineRevisionList(revisions)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_pipeline_revision<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((pipeline_id, revision)): Path<(Uuid, i64)>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::View)
        .await
    {
        return reply;
    }
    match service.revision(pipeline_id, revision).await {
        Ok(Some(found)) => (StatusCode::OK, Json(ApiResponse::PipelineRevision(found))),
        Ok(None) => not_found(format!("Pipeline {pipeline_id} has no revision {revision}")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_pipeline<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(mut pipeline): ValidatedJson<Pipeline>,
) -> (StatusCode, Json<ApiResponse>) {
    // a create always mints a fresh id and is owned by the creator's active org (None = global).
    pipeline.id = Some(Uuid::now_v7());
    pipeline.org_id = ctx.org_id;
    match service.save(&pipeline).await {
        Ok(pipeline) => {
            if let Some(id) = pipeline.id
                && let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                    .grant_pipeline_owner(id)
                    .await
            {
                return reply;
            }
            (StatusCode::OK, Json(ApiResponse::Pipeline(pipeline)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub async fn update_pipeline<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    ValidatedJson(pipeline): ValidatedJson<Pipeline>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::Edit)
        .await
    {
        return reply;
    }
    match service.update(pipeline_id, pipeline).await {
        Ok(Some(pipeline)) => (StatusCode::OK, Json(ApiResponse::Pipeline(pipeline))),
        Ok(None) => not_found(format!("Pipeline {pipeline_id} not found")),
        Err(err) => bad_request(err.to_string()),
    }
}

pub async fn delete_pipeline<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::Edit)
        .await
    {
        return reply;
    }
    match service.delete(pipeline_id, ctx.org_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

// --- pipeline triggers ---

pub async fn get_pipeline_triggers<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::View)
        .await
    {
        return reply;
    }
    match service.list_triggers(pipeline_id).await {
        Ok(triggers) => (
            StatusCode::OK,
            Json(ApiResponse::PipelineTriggerList(triggers)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn upsert_pipeline_trigger<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    ValidatedJson(mut trigger): ValidatedJson<PipelineTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::Edit)
        .await
    {
        return reply;
    }
    trigger.pipeline_id = pipeline_id;
    match service.save_trigger(&trigger, ctx.org_id).await {
        Ok(trigger) => (StatusCode::OK, Json(ApiResponse::PipelineTrigger(trigger))),
        Err(err) => bad_request(err.to_string()),
    }
}

pub async fn update_pipeline_trigger<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
    ValidatedJson(mut trigger): ValidatedJson<PipelineTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_trigger(trigger_id, Permission::Edit)
        .await
    {
        return reply;
    }
    trigger.id = Some(trigger_id);
    match service.save_trigger(&trigger, ctx.org_id).await {
        Ok(trigger) => (StatusCode::OK, Json(ApiResponse::PipelineTrigger(trigger))),
        Err(err) => bad_request(err.to_string()),
    }
}

pub async fn delete_pipeline_trigger<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_trigger(trigger_id, Permission::Edit)
        .await
    {
        return reply;
    }
    match service.delete_trigger(trigger_id, ctx.org_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

// --- pipeline runs ---

/// Admit a generic external event before starting a pipeline. A repeated correlation key is
/// rejected before any run is created, even when the event reaches another web-service replica.
pub async fn ingress_pipeline_run<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + RbacStore
        + OrchestrationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<IngressEventRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::Run)
        .await
    {
        return reply;
    }
    process_pipeline_ingress(service, pipeline_id, ctx.org_id, request, None).await
}

/// Provider-neutral ingress core shared by authenticated direct calls and verified adapter events.
pub async fn process_pipeline_ingress<
    T: DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + RbacStore
        + OrchestrationStore,
>(
    service: Arc<PipelineOperations<T>>,
    pipeline_id: Uuid,
    caller_org_id: Option<Uuid>,
    request: IngressEventRequest,
    adapter: Option<(Uuid, i64)>,
) -> (StatusCode, Json<ApiResponse>) {
    match service
        .process_ingress(
            pipeline_id,
            caller_org_id,
            PipelineIngressRequest {
                source: request.source,
                event_id: request.event_id,
                event_type: request.event_type,
                correlation_key: request.correlation_key,
                payload: request.payload,
                provenance: request.provenance,
                occurred_at: request.occurred_at,
            },
            adapter,
        )
        .await
    {
        Ok(value) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::Ingress(IngressResponse {
                admission_id: value.admission_id,
                generation: value.generation,
                disposition: value.disposition,
                duplicate: value.duplicate,
                queue_position: value.queue_position,
                workflow_run_id: value.workflow_run_id,
                pipeline_run_id: value.pipeline_run_id,
                orchestration_binding_id: value.orchestration_binding_id,
                message: value.message,
            })),
        ),
        Err(PipelineIngressError::Held(record)) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::ExternalIngressRecord(*record)),
        ),
        Err(PipelineIngressError::Full) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ApiResponse::ApiError(ApiError {
                message: "ingress review queue is full; retry later".into(),
                path: None,
                expected: None,
                actual: None,
            })),
        ),
        Err(PipelineIngressError::NotFound(message)) => not_found(message),
        Err(PipelineIngressError::Invalid(message)) => bad_request(message),
        Err(PipelineIngressError::Conflict(message)) => pipeline_ingress_conflict(message),
        Err(PipelineIngressError::Internal(message)) => api_error(message),
    }
}

fn pipeline_ingress_conflict(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::CONFLICT,
        Json(ApiResponse::ApiError(ApiError::new(message))),
    )
}

pub async fn create_pipeline_run<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<PipelineRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::Run)
        .await
    {
        return reply;
    }
    match service
        .create_run(
            pipeline_id,
            request.parameters,
            request.revision,
            Some("api".into()),
            request.start_member,
        )
        .await
    {
        Ok(run) => (StatusCode::ACCEPTED, Json(ApiResponse::PipelineRun(run))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_pipeline_trigger_run<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<PipelineRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_trigger(trigger_id, Permission::Run)
        .await
    {
        return reply;
    }
    match service
        .create_run_for_trigger(trigger_id, request.parameters, Some("api".into()))
        .await
    {
        Ok(run) => (StatusCode::ACCEPTED, Json(ApiResponse::PipelineRun(run))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_pipeline_runs<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    match service.list_recent_runs(200).await {
        Ok(runs) => {
            let visible = AuthzChecker::new(db.as_ref(), &ctx)
                .visible_pipeline_ids()
                .await;
            let runs = match visible {
                Ok(Some(ids)) => runs
                    .into_iter()
                    .filter(|run| ids.contains(&run.pipeline_id))
                    .collect(),
                Ok(None) => runs,
                Err(reply) => return reply,
            };
            (StatusCode::OK, Json(ApiResponse::PipelineRunList(runs)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_pipeline_run<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::View)
        .await
    {
        return reply;
    }
    match service.fetch_run_detail(pipeline_run_id).await {
        Ok(Some(detail)) => (StatusCode::OK, Json(ApiResponse::PipelineRunDetail(detail))),
        Ok(None) => not_found(format!("Pipeline run {pipeline_run_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_pipeline_run<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Edit)
        .await
    {
        return reply;
    }
    if let Err(reply) = require_unmanaged_pipeline_run(service.as_ref(), pipeline_run_id).await {
        return reply;
    }
    match service.delete_run(pipeline_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn cancel_pipeline_run<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
    body: Option<ValidatedJson<ManagedRunOverrideRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    let override_request = body.as_ref().map(|ValidatedJson(request)| request);
    if let Err(reply) = authorize_pipeline_run_control(
        db.clone(),
        service.as_ref(),
        &ctx,
        pipeline_run_id,
        "cancel",
        override_request,
    )
    .await
    {
        return reply;
    }
    match service.cancel_run(pipeline_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn pause_pipeline_run<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
    body: Option<ValidatedJson<ManagedRunOverrideRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    let override_request = body.as_ref().map(|ValidatedJson(request)| request);
    if let Err(reply) = authorize_pipeline_run_control(
        db.clone(),
        service.as_ref(),
        &ctx,
        pipeline_run_id,
        "pause",
        override_request,
    )
    .await
    {
        return reply;
    }
    match service.pause_run(pipeline_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn resume_pipeline_run<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
    body: Option<ValidatedJson<ManagedRunOverrideRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    let override_request = body.as_ref().map(|ValidatedJson(request)| request);
    if let Err(reply) = authorize_pipeline_run_control(
        db.clone(),
        service.as_ref(),
        &ctx,
        pipeline_run_id,
        "resume",
        override_request,
    )
    .await
    {
        return reply;
    }
    match service.resume_run(pipeline_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

/// resolve a pipeline run's pending inquiry: a member whose failure mode is `Inquire` paused the run
/// until a human decides whether to continue (fire that member's onward pipeline links and resume)
/// or abort (settle the run `failed` now).
pub async fn resolve_pipeline_run<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<PipelineRunResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    let override_request = ManagedRunOverrideRequest {
        reason: request.override_reason.clone(),
        idempotency_key: request.idempotency_key.clone(),
    };
    if let Err(reply) = authorize_pipeline_run_control(
        db.clone(),
        service.as_ref(),
        &ctx,
        pipeline_run_id,
        "resolve_inquiry",
        Some(&override_request),
    )
    .await
    {
        return reply;
    }
    let continue_pipeline = request.decision == PipelineRunInquiryDecision::Continue;
    match service
        .resolve_run_inquiry(
            pipeline_run_id,
            continue_pipeline,
            request.resolved_by,
            request.message,
        )
        .await
    {
        Ok(run) => (StatusCode::OK, Json(ApiResponse::PipelineRun(run))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn retry_pipeline_member<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((pipeline_run_id, member_key)): Path<(Uuid, String)>,
    ValidatedJson(request): ValidatedJson<PipelineMemberRetryRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    let override_request = ManagedRunOverrideRequest {
        reason: request.override_reason.clone(),
        idempotency_key: request.idempotency_key.clone(),
    };
    if let Err(reply) = authorize_pipeline_run_control(
        db.clone(),
        service.as_ref(),
        &ctx,
        pipeline_run_id,
        &format!("retry_member:{member_key}"),
        Some(&override_request),
    )
    .await
    {
        return reply;
    }
    match service
        .retry_member(pipeline_run_id, member_key, request.parameters)
        .await
    {
        Ok(attempt) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::PipelineMemberAttempt(attempt)),
        ),
        Err(err) => (
            StatusCode::CONFLICT,
            Json(ApiResponse::ApiError(
                runinator_ws_core::models::ApiError::new(err.to_string()),
            )),
        ),
    }
}

#[allow(clippy::result_large_err)]
async fn require_unmanaged_pipeline_run<
    T: DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    service: &PipelineOperations<T>,
    pipeline_run_id: Uuid,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    match service.fetch_run_detail(pipeline_run_id).await {
        Ok(Some(detail)) if detail.run.orchestration_binding_id.is_some() => Err(bad_request(
            "This pipeline run is managed by a correlated orchestration; send a named intent to the orchestration instead",
        )),
        Ok(_) => Ok(()),
        Err(error) => Err(api_error(error.to_string())),
    }
}

#[allow(clippy::result_large_err)]
async fn authorize_pipeline_run_control<
    T: DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
>(
    db: Arc<T>,
    service: &PipelineOperations<T>,
    ctx: &AuthContext,
    pipeline_run_id: Uuid,
    action: &str,
    request: Option<&ManagedRunOverrideRequest>,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    let binding_id = match service.fetch_run_detail(pipeline_run_id).await {
        Ok(Some(detail)) => detail.run.orchestration_binding_id,
        Ok(None) => return Ok(()),
        Err(error) => return Err(api_error(error.to_string())),
    };
    let Some(binding_id) = binding_id else {
        return Ok(());
    };
    if !ctx.is_platform_admin() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiResponse::ApiError(ApiError::new(
                "Only a platform administrator may force a managed pipeline run control",
            ))),
        ));
    }
    let reason = request
        .and_then(|request| request.reason.as_deref())
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .ok_or_else(|| bad_request("A force override requires a non-empty reason"))?;
    let idempotency_key = request
        .and_then(|request| request.idempotency_key.as_deref())
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| bad_request("A force override requires an idempotency key"))?;
    let binding = db
        .fetch_orchestration_binding(binding_id)
        .await
        .map_err(|error| api_error(error.to_string()))?
        .ok_or_else(|| not_found("managed orchestration binding not found"))?;
    let record = OrchestrationOperations::new(db)
        .record_out_of_band_override(
            &binding,
            "pipeline_run",
            pipeline_run_id,
            action,
            reason.to_owned(),
            idempotency_key.to_owned(),
            ctx.principal_id,
        )
        .await
        .map_err(|error| api_error(error.to_string()))?;
    if record.duplicate {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiResponse::ApiError(ApiError::new(
                "This force override idempotency key was already used",
            ))),
        ));
    }
    Ok(())
}

/// the `pipelines` endpoints.
pub fn routes<
    T: AuthorizationStore
        + DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, patch, post};
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_PIPELINES,
            get(get_pipelines::<T>)
                .post(create_pipeline::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/pipelines/{id}",
            get(get_pipeline::<T>)
                .patch(update_pipeline::<T>)
                .delete(delete_pipeline::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/pipelines/{id}/revisions",
            get(get_pipeline_revisions::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipelines/{id}/revisions/{revision}",
            get(get_pipeline_revision::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipelines/{id}/triggers",
            get(get_pipeline_triggers::<T>)
                .post(upsert_pipeline_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_triggers/{id}",
            patch(update_pipeline_trigger::<T>)
                .delete(delete_pipeline_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_triggers/{id}/runs",
            post(create_pipeline_trigger_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipelines/{id}/runs",
            post(create_pipeline_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipelines/{id}/ingress",
            post(ingress_pipeline_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_runs",
            get(get_pipeline_runs::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_runs/{id}",
            get(get_pipeline_run::<T>)
                .delete(delete_pipeline_run::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_runs/{id}/cancel",
            post(cancel_pipeline_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_runs/{id}/pause",
            post(pause_pipeline_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_runs/{id}/resume",
            post(resume_pipeline_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_runs/{id}/resolve",
            post(resolve_pipeline_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/pipeline_runs/{id}/members/{member_key}/retry",
            post(retry_pipeline_member::<T>).layer(Extension(pool.clone())),
        )
}
