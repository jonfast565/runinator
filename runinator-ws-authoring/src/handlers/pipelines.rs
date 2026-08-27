use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_models::{
    auth::{AuthContext, Permission},
    orchestration::{
        IngressAction, IngressAdmissionClaim, IngressAdmissionStatus, IngressEvent,
        IngressEventDisposition, IngressInboxEntry, IngressLifecycle, IngressPolicy, IngressTarget,
        IngressTargetKind,
    },
    pipelines::{Pipeline, PipelineTrigger},
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, IngressStore, OrchestrationStore, ScheduleStore, WorkflowVmStore},
};
use serde::Deserialize;

use runinator_engine::services::{IngressOperations, OrchestrationOperations, PipelineOperations};
use runinator_ws_core::models::{
    ApiError, ApiResponse, IngressEventRequest, IngressResponse, PipelineMemberRetryRequest,
    PipelineRunInquiryDecision, PipelineRunRequest, PipelineRunResolutionRequest,
};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};

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
    Json(mut pipeline): Json<Pipeline>,
) -> (StatusCode, Json<ApiResponse>) {
    // a create always mints a fresh id and is owned by the creator's active org (None = global).
    pipeline.id = None;
    pipeline.org_id = ctx.org_id;
    match service.save(&pipeline).await {
        Ok(pipeline) => {
            if let Some(id) = pipeline.id {
                if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                    .grant_pipeline_owner(id)
                    .await
                {
                    return reply;
                }
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
    Json(pipeline): Json<Pipeline>,
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
    Json(mut trigger): Json<PipelineTrigger>,
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
    Json(mut trigger): Json<PipelineTrigger>,
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
        + OrchestrationStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    Json(request): Json<IngressEventRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline(pipeline_id, Permission::Run)
        .await
    {
        return reply;
    }
    process_pipeline_ingress(db, service, pipeline_id, ctx.org_id, request, None).await
}

/// Provider-neutral ingress core shared by authenticated direct calls and verified adapter events.
pub async fn process_pipeline_ingress<
    T: DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
>(
    db: Arc<T>,
    service: Arc<PipelineOperations<T>>,
    pipeline_id: Uuid,
    caller_org_id: Option<Uuid>,
    request: IngressEventRequest,
    adapter: Option<(Uuid, i64)>,
) -> (StatusCode, Json<ApiResponse>) {
    let pipeline = match service.fetch(pipeline_id).await {
        Ok(Some(pipeline)) => pipeline,
        Ok(None) => return not_found("pipeline not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let policy = match pipeline.metadata.get("ingress") {
        Some(value) => match serde_json::from_value::<IngressPolicy>(value.clone().into()) {
            Ok(policy) => policy,
            Err(error) => return bad_request(format!("invalid pipeline ingress policy: {error}")),
        },
        None => return bad_request("pipeline has no ingress policy"),
    };
    let event = IngressEvent {
        source: request.source,
        event_id: request.event_id,
        event_type: request.event_type,
        correlation_key: request.correlation_key,
        payload: request.payload,
        occurred_at: request.occurred_at,
    };
    let ingress = IngressOperations::new(db.clone());
    let target = IngressTarget {
        kind: IngressTargetKind::Pipeline,
        id: pipeline_id,
    };
    let org_id = pipeline.org_id.or(caller_org_id);
    let mut admission = match ingress
        .fetch(org_id, policy.scope.clone(), event.correlation_key.clone())
        .await
    {
        Ok(value) => value,
        Err(err) => return api_error(err.to_string()),
    };
    let mut start_record = None;
    if admission.is_none() {
        match ingress
            .claim_start(org_id, target, policy.clone(), &event)
            .await
        {
            Ok(Some(IngressAdmissionClaim::Acquired(value))) => {
                start_record = match ingress
                    .persist_event(&value, &event, IngressEventDisposition::Started, false)
                    .await
                {
                    Ok(record) => Some(record.entry),
                    Err(err) => {
                        let _ = ingress.release_unbound(value.id.unwrap()).await;
                        return api_error(err.to_string());
                    }
                };
                admission = Some(value);
            }
            Ok(Some(IngressAdmissionClaim::Existing(value))) => admission = Some(value),
            Ok(None) => {
                return bad_request(
                    "ingress event has no configured unbound start route; no run was started",
                );
            }
            Err(err) => return bad_request(err.to_string()),
        }
    }
    let mut admission = admission.expect("ingress admission resolved");
    let admission_id = admission.id.expect("stored admission id");
    if start_record.is_none() {
        match ingress.duplicate(admission_id, &event).await {
            Ok(Some(entry)) => {
                return pipeline_ingress_reply(&entry, true, "duplicate ingress event");
            }
            Ok(None) => {}
            Err(err) => return api_error(err.to_string()),
        }
        if admission.target.kind != IngressTargetKind::Pipeline
            || admission.target.id != pipeline_id
        {
            return pipeline_ingress_conflict(
                "this scope and correlation key is owned by a different ingress target",
            );
        }
        let snapshot_policy: IngressPolicy =
            match serde_json::from_value(admission.policy.clone().into()) {
                Ok(value) => value,
                Err(err) => return api_error(format!("stored ingress policy is invalid: {err}")),
            };
        let lifecycle = match admission.status {
            IngressAdmissionStatus::Active => IngressLifecycle::Active,
            IngressAdmissionStatus::Terminal => IngressLifecycle::Terminal,
        };
        let dispatched =
            snapshot_policy.dispatches_for(&event.event_type, lifecycle, &event.payload);
        if !dispatched.is_empty() {
            return match ingress
                .persist_event(&admission, &event, IngressEventDisposition::Recorded, false)
                .await
            {
                Ok(record) => pipeline_ingress_reply(
                    &record.entry,
                    record.duplicate,
                    "orchestration intent event accepted",
                ),
                Err(err) => api_error(err.to_string()),
            };
        }
        match snapshot_policy.action_for(&event.event_type, lifecycle) {
            Some(IngressAction::Record) => {
                return match ingress
                    .persist_event(&admission, &event, IngressEventDisposition::Recorded, false)
                    .await
                {
                    Ok(record) => pipeline_ingress_reply(
                        &record.entry,
                        record.duplicate,
                        "ingress event recorded",
                    ),
                    Err(err) => api_error(err.to_string()),
                };
            }
            Some(IngressAction::Queue) if lifecycle == IngressLifecycle::Active => {
                return match ingress
                    .persist_event(&admission, &event, IngressEventDisposition::Queued, true)
                    .await
                {
                    Ok(record) => pipeline_ingress_reply(
                        &record.entry,
                        record.duplicate,
                        "ingress event queued",
                    ),
                    Err(err) => api_error(err.to_string()),
                };
            }
            Some(IngressAction::Interrupt) if lifecycle == IngressLifecycle::Active => {
                let Some(run_id) = admission.pipeline_run_id else {
                    return api_error("active ingress admission is not bound to a pipeline run");
                };
                let record = match ingress
                    .persist_event(
                        &admission,
                        &event,
                        IngressEventDisposition::InterruptRequested,
                        false,
                    )
                    .await
                {
                    Ok(record) => record,
                    Err(err) => return api_error(err.to_string()),
                };
                if record.duplicate {
                    return pipeline_ingress_reply(
                        &record.entry,
                        true,
                        "duplicate pipeline interrupt event",
                    );
                }
                let _ = ingress
                    .bind_event_pipeline_run(record.entry.id, run_id)
                    .await;
                return match service.cancel_run(run_id).await {
                    Ok(_) => pipeline_ingress_reply(
                        &record.entry,
                        record.duplicate,
                        "pipeline and active members canceled",
                    ),
                    Err(err) => bad_request(err.to_string()),
                };
            }
            Some(IngressAction::Requeue) if lifecycle == IngressLifecycle::Terminal => {
                match ingress
                    .requeue_terminal_event(&admission, &snapshot_policy, &event)
                    .await
                {
                    Ok(Some(record)) if record.duplicate => {
                        return pipeline_ingress_reply(
                            &record.entry,
                            true,
                            "duplicate terminal requeue event",
                        );
                    }
                    Ok(Some(record)) => {
                        admission = match ingress
                            .fetch(
                                org_id,
                                snapshot_policy.scope.clone(),
                                event.correlation_key.clone(),
                            )
                            .await
                        {
                            Ok(Some(value)) => value,
                            Ok(None) => return api_error("requeued ingress admission disappeared"),
                            Err(err) => return api_error(err.to_string()),
                        };
                        start_record = Some(record.entry);
                    }
                    Ok(None) => {
                        return pipeline_ingress_conflict(
                            "another ingress event already started the next generation",
                        );
                    }
                    Err(err) => return api_error(err.to_string()),
                }
            }
            _ => {
                let _ = ingress
                    .persist_event(&admission, &event, IngressEventDisposition::Rejected, false)
                    .await;
                return pipeline_ingress_conflict(
                    "ingress event has no configured route for the admission lifecycle; no run was started",
                );
            }
        }
    }
    let start_entry = start_record.expect("start event record");
    if pipeline.metadata.get("orchestration").is_some() {
        let orchestrations = OrchestrationOperations::new(db.clone());
        return match orchestrations
            .admit_with_adapter(&admission, &pipeline, adapter)
            .await
        {
            Ok(Some(binding)) => {
                if let Some((adapter_id, _)) = adapter {
                    if let Err(error) = db
                        .mark_orchestration_adapter_admitted(adapter_id, Utc::now())
                        .await
                    {
                        return api_error(error.to_string());
                    }
                }
                pipeline_orchestration_ingress_reply(
                    &start_entry,
                    binding.id,
                    "managed orchestration generation admitted",
                )
            }
            Ok(None) => api_error("managed orchestration policy disappeared"),
            Err(err) => api_error(err.to_string()),
        };
    }
    match service
        .create_run(
            admission.target.id,
            event.payload.clone(),
            None,
            Some(format!("ingress:{}", event.event_id)),
            None,
        )
        .await
    {
        Ok(run) => match ingress.bind_pipeline_run(admission_id, run.id).await {
            Ok(true) => {
                let _ = ingress
                    .bind_event_pipeline_run(start_entry.id, run.id)
                    .await;
                let mut entry = start_entry;
                entry.pipeline_run_id = Some(run.id);
                pipeline_ingress_reply(&entry, false, "pipeline ingress generation started")
            }
            Ok(false) => api_error("ingress admission could not be bound to the pipeline run"),
            Err(err) => api_error(err.to_string()),
        },
        Err(err) => {
            let _ = ingress.release_unbound(admission_id).await;
            api_error(err.to_string())
        }
    }
}

fn pipeline_ingress_reply(
    entry: &IngressInboxEntry,
    duplicate: bool,
    message: &str,
) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::ACCEPTED,
        Json(ApiResponse::Ingress(IngressResponse {
            admission_id: entry.admission_id,
            generation: entry.promoted_generation.unwrap_or(entry.generation),
            disposition: format!("{:?}", entry.disposition).to_ascii_lowercase(),
            duplicate,
            queue_position: entry.queue_position,
            workflow_run_id: entry.workflow_run_id,
            pipeline_run_id: entry.pipeline_run_id,
            orchestration_binding_id: None,
            message: message.into(),
        })),
    )
}

fn pipeline_orchestration_ingress_reply(
    entry: &IngressInboxEntry,
    binding_id: Uuid,
    message: &str,
) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::ACCEPTED,
        Json(ApiResponse::Ingress(IngressResponse {
            admission_id: entry.admission_id,
            generation: entry.promoted_generation.unwrap_or(entry.generation),
            disposition: "started".into(),
            duplicate: false,
            queue_position: entry.queue_position,
            workflow_run_id: None,
            pipeline_run_id: None,
            orchestration_binding_id: Some(binding_id),
            message: message.into(),
        })),
    )
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
    Json(request): Json<PipelineRunRequest>,
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
    Json(request): Json<PipelineRunRequest>,
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
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    if let Err(reply) = require_unmanaged_pipeline_run(service.as_ref(), pipeline_run_id).await {
        return reply;
    }
    match service.cancel_run(pipeline_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn pause_pipeline_run<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    if let Err(reply) = require_unmanaged_pipeline_run(service.as_ref(), pipeline_run_id).await {
        return reply;
    }
    match service.pause_run(pipeline_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn resume_pipeline_run<
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    if let Err(reply) = require_unmanaged_pipeline_run(service.as_ref(), pipeline_run_id).await {
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
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
    Json(request): Json<PipelineRunResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    if let Err(reply) = require_unmanaged_pipeline_run(service.as_ref(), pipeline_run_id).await {
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
    T: AuthorizationStore + DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((pipeline_run_id, member_key)): Path<(Uuid, String)>,
    Json(request): Json<PipelineMemberRetryRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_pipeline_run(pipeline_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    if let Err(reply) = require_unmanaged_pipeline_run(service.as_ref(), pipeline_run_id).await {
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
