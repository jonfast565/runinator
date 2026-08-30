use std::{net::SocketAddr, sync::Arc};
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_models::{
    interrupt::InterruptSource,
    orchestration::{
        IngressAction, IngressAdmissionClaim, IngressAdmissionStatus, IngressEvent,
        IngressEventDisposition, IngressInboxEntry, IngressLifecycle, IngressPolicy, IngressTarget,
        IngressTargetKind,
    },
    replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance},
};
use runinator_store::{
    RuntimeStore,
    roles::{
        FileStore, IngressStore, OrchestrationStore, RunStore, ScheduleStore, WorkflowVmStore,
    },
};

use runinator_engine::services::{IngressOperations, OrchestrationOperations, RunOperations};
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::models::{
    self, ApiError, ApiResponse, IngressAdmissionQuery, IngressEventRequest, IngressResponse,
    ManagedRunOverrideRequest, SchedulerRunClaimReleaseRequest, SchedulerRunClaimRenewRequest,
    SchedulerRunClaimRequest, TaskResponseSchema, WorkflowRunRequest, WorkflowRunStatusQuery,
    WorkflowRunStatusRequest, WorkflowTriggerRunRequest,
};
use runinator_ws_core::openapi::docs::{
    EndpointDoc, Example, ParamDoc, WORKFLOW_RUN_FILTERS, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::AuthContextExt;
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};

/// Persistence the workflow-run HTTP surface coordinates. It excludes authoring, settings,
/// notifications, functions, and replica management while keeping the cross-domain run commands
/// atomic at the handler boundary.
pub trait RunOperationsStore:
    AuthorizationStore
    + RuntimeStore
    + WorkflowVmStore
    + RunStore
    + ScheduleStore
    + FileStore
    + IngressStore
    + OrchestrationStore
{
}

impl<T> RunOperationsStore for T where
    T: AuthorizationStore
        + RuntimeStore
        + WorkflowVmStore
        + RunStore
        + ScheduleStore
        + FileStore
        + IngressStore
        + OrchestrationStore
{
}

pub async fn create_workflow_trigger_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    _headers: HeaderMap,
    _connect: ConnectInfo<SocketAddr>,
    Path(trigger_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<WorkflowTriggerRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_trigger_workflow(trigger_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match operations
        .create_for_trigger(
            trigger_id,
            request.parameters,
            request.debug,
            None,
            Some(request_actor_display_name()),
        )
        .await
    {
        Ok(run) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
                run,
                Vec::new(),
            ))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Path(workflow_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<WorkflowRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match operations
        .create(
            workflow_id,
            request.parameters,
            request.debug,
            request.name,
            request_provenance(
                TriggerSourceKind::Api,
                &headers,
                connect,
                runinator_models::json!({}),
            ),
            request.file_ids,
            ctx.org_id,
            ctx.principal_id,
        )
        .await
    {
        Ok(run) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
                run,
                Vec::new(),
            ))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// Admit an opaque ingress event before creating a workflow run. The durable admission is shared
/// with pipeline ingress, so targets that intentionally name the same scope exclude one another.
pub async fn ingress_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Path(workflow_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<IngressEventRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    let workflow = match operations.fetch_workflow_definition(workflow_id).await {
        Ok(Some(workflow)) => workflow,
        Ok(None) => return not_found("workflow not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let policy = match workflow.definition.metadata.get("ingress") {
        Some(value) => match serde_json::from_value::<IngressPolicy>(value.clone().into()) {
            Ok(policy) => policy,
            Err(error) => return bad_request(format!("invalid workflow ingress policy: {error}")),
        },
        None => return bad_request("workflow has no ingress policy"),
    };
    let event = IngressEvent {
        source: request.source,
        event_id: request.event_id,
        event_type: request.event_type,
        correlation_key: request.correlation_key,
        payload: request.payload,
        provenance: request.provenance,
        occurred_at: request.occurred_at,
    };
    let ingress = IngressOperations::new(db.clone());
    let requested_target = IngressTarget {
        kind: IngressTargetKind::Workflow,
        id: workflow_id,
    };
    let org_id = workflow.org_id.or(ctx.org_id);
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
            .claim_start(org_id, requested_target.clone(), policy.clone(), &event)
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
            Ok(Some(entry)) => return ingress_event_reply(&entry, true, "duplicate ingress event"),
            Ok(None) => {}
            Err(err) => return api_error(err.to_string()),
        }
        if admission.target.kind != IngressTargetKind::Workflow
            || admission.target.id != workflow_id
        {
            return conflict(
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
        match snapshot_policy.action_for_payload(&event.event_type, lifecycle, &event.payload) {
            Some(IngressAction::Record) => {
                return match ingress
                    .persist_event(&admission, &event, IngressEventDisposition::Recorded, false)
                    .await
                {
                    Ok(record) => ingress_event_reply(
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
                    Ok(record) => {
                        ingress_event_reply(&record.entry, record.duplicate, "ingress event queued")
                    }
                    Err(err) => api_error(err.to_string()),
                };
            }
            Some(IngressAction::Interrupt) if lifecycle == IngressLifecycle::Active => {
                let Some(run_id) = admission.workflow_run_id else {
                    return api_error("active ingress admission is not bound to a workflow run");
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
                    return ingress_event_reply(
                        &record.entry,
                        true,
                        "duplicate workflow interrupt event",
                    );
                }
                let _ = ingress
                    .bind_event_workflow_run(record.entry.id, run_id)
                    .await;
                return match operations
                    .request_interrupt(
                        run_id,
                        InterruptSource::External,
                        event.payload.clone(),
                        None,
                    )
                    .await
                {
                    Ok(_) => ingress_event_reply(
                        &record.entry,
                        record.duplicate,
                        "workflow interrupt requested",
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
                        return ingress_event_reply(
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
                        return conflict(
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
                return conflict(
                    "ingress event has no configured route for the admission lifecycle; no run was started",
                );
            }
        }
    }
    let start_entry = start_record.expect("start event record");
    match operations
        .create(
            admission.target.id,
            event.payload.clone(),
            false,
            Some(format!("ingress:{}", event.event_id)),
            request_provenance(
                TriggerSourceKind::Api,
                &headers,
                connect,
                runinator_models::json!({
                    "ingress_source": event.source,
                    "ingress_event_id": event.event_id,
                }),
            ),
            Vec::new(),
            workflow.org_id.or(ctx.org_id),
            ctx.principal_id,
        )
        .await
    {
        Ok(run) => match ingress.bind_workflow_run(admission_id, run.id).await {
            Ok(true) => {
                let _ = ingress
                    .bind_event_workflow_run(start_entry.id, run.id)
                    .await;
                let mut entry = start_entry;
                entry.workflow_run_id = Some(run.id);
                ingress_event_reply(&entry, false, "workflow ingress generation started")
            }
            Ok(false) => api_error("ingress admission could not be bound to the workflow run"),
            Err(err) => api_error(err.to_string()),
        },
        Err(err) => {
            let _ = ingress.release_unbound(admission_id).await;
            api_error(err.to_string())
        }
    }
}

fn ingress_event_reply(
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

fn conflict(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::CONFLICT,
        Json(ApiResponse::ApiError(ApiError::new(message))),
    )
}

pub async fn get_ingress_admission<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Query(query): Query<IngressAdmissionQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let ingress = IngressOperations::new(db.clone());
    let admission = match ingress
        .fetch(ctx.org_id, query.scope, query.correlation_key)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return not_found("ingress admission not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let checker = AuthzChecker::new(db.as_ref(), &ctx);
    let authorized = match admission.target.kind {
        IngressTargetKind::Workflow => {
            checker
                .require_workflow(
                    admission.target.id,
                    runinator_models::auth::Permission::View,
                )
                .await
        }
        IngressTargetKind::Pipeline => {
            checker
                .require_pipeline(
                    admission.target.id,
                    runinator_models::auth::Permission::View,
                )
                .await
        }
    };
    if let Err(reply) = authorized {
        return reply;
    }
    (
        StatusCode::OK,
        Json(ApiResponse::IngressAdmission(admission)),
    )
}

pub async fn get_ingress_timeline<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Query(query): Query<IngressAdmissionQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let ingress = IngressOperations::new(db.clone());
    let admission = match ingress
        .fetch(ctx.org_id, query.scope, query.correlation_key)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return not_found("ingress admission not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let checker = AuthzChecker::new(db.as_ref(), &ctx);
    let authorized = match admission.target.kind {
        IngressTargetKind::Workflow => {
            checker
                .require_workflow(
                    admission.target.id,
                    runinator_models::auth::Permission::View,
                )
                .await
        }
        IngressTargetKind::Pipeline => {
            checker
                .require_pipeline(
                    admission.target.id,
                    runinator_models::auth::Permission::View,
                )
                .await
        }
    };
    if let Err(reply) = authorized {
        return reply;
    }
    match ingress
        .timeline(admission.id.expect("stored admission id"))
        .await
    {
        Ok(events) => (StatusCode::OK, Json(ApiResponse::IngressTimeline(events))),
        Err(err) => api_error(err.to_string()),
    }
}

fn request_provenance(
    source_kind: TriggerSourceKind,
    headers: &HeaderMap,
    connect: SocketAddr,
    metadata: runinator_models::value::Value,
) -> WorkflowRunProvenance {
    WorkflowRunProvenance {
        source_kind: Some(source_kind),
        actor_type: Some(TriggerActorType::User),
        actor_replica_id: None,
        actor_display_name: Some(request_actor_display_name()),
        request_host: headers
            .get("host")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
        request_ip: headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| Some(connect.ip().to_string())),
        metadata,
    }
}

fn request_actor_display_name() -> String {
    "api".into()
}

pub async fn claim_workflow_runs_for_scheduler<T: RunOperationsStore>(
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    ValidatedJson(request): ValidatedJson<SchedulerRunClaimRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    let statuses = if request.statuses.is_empty() {
        vec![
            runinator_models::workflows::WorkflowStatus::Queued,
            runinator_models::workflows::WorkflowStatus::Running,
            runinator_models::workflows::WorkflowStatus::DebugPaused,
            runinator_models::workflows::WorkflowStatus::Waiting,
            runinator_models::workflows::WorkflowStatus::Parked,
            runinator_models::workflows::WorkflowStatus::Sleeping,
            runinator_models::workflows::WorkflowStatus::ApprovalRequired,
            runinator_models::workflows::WorkflowStatus::InputRequired,
            runinator_models::workflows::WorkflowStatus::Blocked,
        ]
    } else {
        request.statuses
    };
    match operations
        .claim_for_scheduler(
            request.scheduler_id,
            statuses,
            request.lease_until,
            request.limit.unwrap_or(50),
        )
        .await
    {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn renew_workflow_run_claim<T: RunOperationsStore>(
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<SchedulerRunClaimRenewRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match operations
        .renew_scheduler_claim(workflow_run_id, request.scheduler_id, request.lease_until)
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(
                runinator_models::web::TaskResponse {
                    success: true,
                    message: "Workflow run claim renewed".into(),
                },
            )),
        ),
        Ok(false) => not_found(format!("Workflow run claim {workflow_run_id} not held")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn release_workflow_run_claim<T: RunOperationsStore>(
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<SchedulerRunClaimReleaseRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match operations
        .release_scheduler_claim(workflow_run_id, request.scheduler_id)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(
                runinator_models::web::TaskResponse {
                    success: true,
                    message: "Workflow run claim released".into(),
                },
            )),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/workflow_runs/{id}/cancel",
    tag = "Workflow Runs",
    params(("id" = Uuid, Path, description = "Workflow run identifier.")),
    request_body = Option<runinator_ws_core::models::ManagedRunOverrideRequest>,
    responses(
        (status = 200, description = "workflow run cancel requested", body = TaskResponseSchema),
        (status = 400, description = "workflow run could not be canceled", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn cancel_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    body: Option<ValidatedJson<ManagedRunOverrideRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    let override_request = body.as_ref().map(|ValidatedJson(request)| request);
    if let Err(reply) = authorize_workflow_run_control(
        db.clone(),
        operations.as_ref(),
        &ctx,
        workflow_run_id,
        "cancel",
        override_request,
    )
    .await
    {
        return reply;
    }
    match operations.cancel(workflow_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => bad_request(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/workflow_runs/{id}/pause",
    tag = "Workflow Runs",
    params(("id" = Uuid, Path, description = "Workflow run identifier.")),
    request_body = Option<runinator_ws_core::models::ManagedRunOverrideRequest>,
    responses(
        (status = 200, description = "workflow run pause requested", body = TaskResponseSchema),
        (status = 400, description = "workflow run could not be paused", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn pause_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    body: Option<ValidatedJson<ManagedRunOverrideRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    let override_request = body.as_ref().map(|ValidatedJson(request)| request);
    if let Err(reply) = authorize_workflow_run_control(
        db.clone(),
        operations.as_ref(),
        &ctx,
        workflow_run_id,
        "pause",
        override_request,
    )
    .await
    {
        return reply;
    }
    match operations.pause(workflow_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => bad_request(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/workflow_runs/{id}/resume",
    tag = "Workflow Runs",
    params(("id" = Uuid, Path, description = "Workflow run identifier.")),
    request_body = Option<runinator_ws_core::models::ManagedRunOverrideRequest>,
    responses(
        (status = 200, description = "workflow run resume requested", body = TaskResponseSchema),
        (status = 400, description = "workflow run could not be resumed", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn resume_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    body: Option<ValidatedJson<ManagedRunOverrideRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    let override_request = body.as_ref().map(|ValidatedJson(request)| request);
    if let Err(reply) = authorize_workflow_run_control(
        db.clone(),
        operations.as_ref(),
        &ctx,
        workflow_run_id,
        "resume",
        override_request,
    )
    .await
    {
        return reply;
    }
    match operations.resume(workflow_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => bad_request(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/workflow_runs/{id}/replay",
    tag = "Workflow Runs",
    params(("id" = Uuid, Path, description = "Workflow run identifier.")),
    request_body = Option<runinator_ws_core::models::WorkflowRunReplayRequest>,
    responses(
        (status = 202, description = "workflow run replay accepted", body = serde_json::Value),
        (status = 400, description = "workflow run could not be replayed", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn replay_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    body: Option<ValidatedJson<runinator_ws_core::models::WorkflowRunReplayRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    let request = body
        .map(|ValidatedJson(request)| request)
        .unwrap_or_default();
    let override_request = ManagedRunOverrideRequest {
        reason: request.override_reason.clone(),
        idempotency_key: request.idempotency_key.clone(),
    };
    if let Err(reply) = authorize_workflow_run_control(
        db.clone(),
        operations.as_ref(),
        &ctx,
        workflow_run_id,
        "replay",
        Some(&override_request),
    )
    .await
    {
        return reply;
    }
    let from_step_id = request.from_step_id;
    match operations.replay(workflow_run_id, from_step_id).await {
        Ok(run) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
                run,
                Vec::new(),
            ))),
        ),
        Err(err) => bad_request(err.to_string()),
    }
}

#[allow(clippy::result_large_err)]
async fn require_unmanaged_workflow_run<T: RunOperationsStore>(
    operations: &RunOperations<T>,
    workflow_run_id: Uuid,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    match operations
        .managed_orchestration_binding(workflow_run_id)
        .await
    {
        Ok(Some(_)) => Err(bad_request(
            "This workflow run belongs to a correlated orchestration; send a named intent to the orchestration instead",
        )),
        Ok(None) => Ok(()),
        Err(error) => return Err(api_error(error.to_string())),
    }
}

#[allow(clippy::result_large_err)]
async fn authorize_workflow_run_control<T: RunOperationsStore>(
    db: Arc<T>,
    operations: &RunOperations<T>,
    ctx: &runinator_models::auth::AuthContext,
    workflow_run_id: Uuid,
    action: &str,
    request: Option<&ManagedRunOverrideRequest>,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    let Some(binding) = operations
        .managed_orchestration_binding(workflow_run_id)
        .await
        .map_err(|error| api_error(error.to_string()))?
    else {
        return Ok(());
    };
    if !ctx.is_platform_admin() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiResponse::ApiError(ApiError::new(
                "Only a platform administrator may force a managed workflow run control",
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
    let record = OrchestrationOperations::new(db)
        .record_out_of_band_override(
            &binding,
            "workflow_run",
            workflow_run_id,
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

/// deliver an event to a parked `event_source` node in one run. the node consumes it on the next
/// drive and re-parks, so repeated deliveries drive repeated iterations of its body.
pub async fn deliver_run_event<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path((workflow_run_id, node_id)): Path<(Uuid, String)>,
    ValidatedJson(request): ValidatedJson<runinator_ws_core::models::EventDeliveryRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    // the VM matches on the event's `type` and evaluates the node's filter against the whole
    // object, so the declared type rides alongside the payload rather than replacing it.
    let mut event = request.data;
    if let (Some(event_type), Some(object)) = (request.event_type, event.as_object_mut()) {
        object.insert("type".into(), event_type.into());
    }
    match operations
        .deliver_event(workflow_run_id, node_id, event)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::TaskResponse(response))),
        Err(err) => bad_request(err.to_string()),
    }
}

pub async fn deliver_signal<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<runinator_ws_core::models::SignalDeliveryRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match operations
        .deliver_signal(workflow_run_id, request.name, request.payload)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::TaskResponse(response))),
        Err(err) => bad_request(err.to_string()),
    }
}

/// ask a run to raise an interrupt, running the handler region declared for that source.
///
/// nothing about serviceability is decided here — the request is recorded on the thread and the VM
/// raises or drops it on the next drive of the target thread. that keeps one copy of the fail-open
/// rules, in the crate that owns them.
pub async fn request_interrupt<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<runinator_ws_core::models::InterruptRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    let raw = request.source.as_deref().unwrap_or("external");
    let Ok(source) = raw.parse::<runinator_models::interrupt::InterruptSource>() else {
        return bad_request(format!("Unknown interrupt source '{raw}'"));
    };
    // only a requested source can be asked for out of band. a drive-matched source (wake, timeout,
    // failure, resolved, child) is classified by the VM from the effect that just settled, so a
    // request for one would sit on the continuation unconsumed and shadow a later genuine request.
    if !source.requested() {
        return bad_request(format!(
            "Interrupt source '{raw}' cannot be requested; it is only raised by a matching drive"
        ));
    }
    match operations
        .request_interrupt(
            workflow_run_id,
            source,
            request.payload,
            request.continuation_id,
        )
        .await
    {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::TaskResponse(response))),
        Err(err) => bad_request(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/workflow_runs/{id}/rename",
    tag = "Workflow Runs",
    params(("id" = Uuid, Path, description = "Workflow run identifier.")),
    request_body = runinator_ws_core::models::WorkflowRunRenameRequest,
    responses(
        (status = 200, description = "workflow run renamed", body = TaskResponseSchema),
        (status = 400, description = "workflow run could not be renamed", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn rename_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<runinator_ws_core::models::WorkflowRunRenameRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Edit)
        .await
    {
        return reply;
    }
    match operations.rename(workflow_run_id, request.name).await {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::TaskResponse(response))),
        Err(err) => bad_request(err.to_string()),
    }
}

/// list workflow runs, optionally filtered by status.
#[utoipa::path(
    get,
    path = "/workflow_runs",
    tag = "Workflow Runs",
    responses((status = 200, description = "workflow runs", body = serde_json::Value)),
)]
pub async fn get_workflow_runs<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Query(query): Query<WorkflowRunStatusQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let visible = match AuthzChecker::new(db.as_ref(), &ctx)
        .visible_workflow_ids()
        .await
    {
        Ok(visible) => visible,
        Err(reply) => return reply,
    };

    if let Some(name) = query.name {
        return match operations
            .list_workflow_by_name(name, query.open.unwrap_or(false))
            .await
        {
            Ok(runs) => (
                StatusCode::OK,
                Json(ApiResponse::WorkflowRunList(filter_runs(runs, &visible))),
            ),
            Err(err) => api_error(err.to_string()),
        };
    }

    if let Some(workflow_id) = query.workflow_id {
        if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_workflow(workflow_id, runinator_models::auth::Permission::View)
            .await
        {
            return reply;
        }
        return match operations.list_workflow_for_definition(workflow_id).await {
            Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
            Err(err) => api_error(err.to_string()),
        };
    }

    if let Some(status) = query.status {
        return match operations.list_workflow_by_status(status).await {
            Ok(runs) => (
                StatusCode::OK,
                Json(ApiResponse::WorkflowRunList(filter_runs(runs, &visible))),
            ),
            Err(err) => api_error(err.to_string()),
        };
    }

    let limit = query
        .limit
        .map(|value| value.clamp(1, MAX_RECENT_RUN_LIMIT))
        .unwrap_or(DEFAULT_RECENT_RUN_LIMIT);
    match operations.list_recent_workflow(limit).await {
        Ok(runs) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowRunList(filter_runs(runs, &visible))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// default cap on the unfiltered recent-runs list, so a long-lived deployment's history doesn't grow
/// the dashboard's poll payload without bound. clients can request more via `?limit=` up to the max.
const DEFAULT_RECENT_RUN_LIMIT: i64 = 200;

/// hard ceiling on `?limit=`, so a client can't ask for an unbounded dump.
const MAX_RECENT_RUN_LIMIT: i64 = 1000;

pub async fn update_workflow_run<T: RunOperationsStore>(
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<WorkflowRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match operations
        .update_workflow_status(
            workflow_run_id,
            request.status,
            request.active_node_id,
            None,
            request.message,
        )
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::View)
        .await
    {
        return reply;
    }
    match operations.fetch_workflow(workflow_run_id).await {
        Ok(Some(run)) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
                run,
                Vec::new(),
            ))),
        ),
        Ok(None) => not_found(format!("Workflow run {workflow_run_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_workflow_run<T: RunOperationsStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(operations): Extension<Arc<RunOperations<T>>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Edit)
        .await
    {
        return reply;
    }
    if let Err(reply) = require_unmanaged_workflow_run(operations.as_ref(), workflow_run_id).await {
        return reply;
    }
    match operations.delete(workflow_run_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

pub fn compute_stale_seconds(updated_at: &str) -> Option<i64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(updated_at).ok()?;
    let now = chrono::Utc::now();
    Some((now - parsed.with_timezone(&chrono::Utc)).num_seconds())
}

fn filter_runs(
    runs: Vec<runinator_models::workflows::WorkflowRun>,
    visible: &Option<std::collections::HashSet<Uuid>>,
) -> Vec<runinator_models::workflows::WorkflowRun> {
    match visible {
        Some(ids) => runs
            .into_iter()
            .filter(|run| ids.contains(&run.workflow_id))
            .collect(),
        None => runs,
    }
}

/// the `runs` endpoints.
pub fn routes<T: RunOperationsStore>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            "/workflow_triggers/{id}/runs",
            post(create_workflow_trigger_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_WORKFLOW_RUNS,
            get(get_workflow_runs::<T>).layer(Extension(pool.clone())),
        )
        .route(
            runinator_models::api_routes::API_SCHEDULER_WORKFLOW_RUNS_CLAIM,
            post(claim_workflow_runs_for_scheduler::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/runs",
            post(create_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/ingress",
            post(ingress_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/ingress/admission",
            get(get_ingress_admission::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/ingress/admission/events",
            get(get_ingress_timeline::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}",
            get(get_workflow_run::<T>)
                .patch(update_workflow_run::<T>)
                .delete(delete_workflow_run::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/workflow_runs/{id}/claim/renew",
            post(renew_workflow_run_claim::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/workflow_runs/{id}/claim/release",
            post(release_workflow_run_claim::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/cancel",
            post(cancel_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/pause",
            post(pause_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/resume",
            post(resume_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/signals",
            post(deliver_signal::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/interrupts",
            post(request_interrupt::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/events/{node_id}",
            post(deliver_run_event::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/replay",
            post(replay_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/rename",
            post(rename_workflow_run::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "post",
        "/workflow_triggers/{id}/runs",
        "Workflow Runs",
        "Start a run from a trigger",
        "Creates a workflow run using a trigger id and the supplied parameters.",
        false,
        json_body("Trigger run parameters.", Example::WorkflowRunRequest),
        &[],
        202,
        "workflow run accepted",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs",
        "Workflow Runs",
        "List workflow runs",
        "Lists recent workflow runs visible to the caller, with optional filters by status, workflow id, name, or open state.",
        false,
        None,
        WORKFLOW_RUN_FILTERS,
        200,
        "workflow runs",
        Example::WorkflowRunList,
    ),
    endpoint(
        "post",
        "/scheduler/workflow_runs/claim",
        "Control Plane",
        "Claim workflow runs for scheduling",
        "Service-control endpoint used by scheduler loops to claim runnable workflow runs with a lease.",
        false,
        json_body(
            "Scheduler id, lease deadline, statuses, and limit.",
            Example::SchedulerRunClaim,
        ),
        &[],
        200,
        "claimed workflow runs",
        Example::WorkflowRunList,
    ),
    endpoint(
        "post",
        "/workflows/{id}/runs",
        "Workflow Runs",
        "Start a workflow run",
        "Creates a workflow run from a workflow definition and supplied parameters.",
        false,
        json_body("Workflow run parameters.", Example::WorkflowRunRequest),
        &[],
        202,
        "workflow run accepted",
        Example::WorkflowRun,
    ),
    endpoint(
        "post",
        "/workflows/{id}/ingress",
        "Ingress",
        "Admit a workflow event",
        "Applies the stored provider-neutral lifecycle policy. Events may start, record, queue, interrupt, or requeue a generation; durable source/event-id retries return the original disposition.",
        false,
        json_body("Opaque ingress event.", Example::IngressEvent),
        &[],
        202,
        "event accepted",
        Example::IngressResponse,
    ),
    endpoint(
        "post",
        "/pipelines/{id}/ingress",
        "Ingress",
        "Admit a pipeline event",
        "Applies the pipeline admission snapshot. Interrupt cancels the pipeline and all active member workflows; queued events are promoted FIFO after settlement.",
        false,
        json_body("Opaque ingress event.", Example::IngressEvent),
        &[],
        202,
        "event accepted",
        Example::IngressResponse,
    ),
    endpoint(
        "get",
        "/ingress/admission",
        "Ingress",
        "Inspect an ingress admission",
        "Returns the sole owner and active generation for an organization, scope, and correlation key.",
        false,
        None,
        INGRESS_LOOKUP_PARAMS,
        200,
        "ingress admission",
        Example::IngressAdmission,
    ),
    endpoint(
        "get",
        "/ingress/admission/events",
        "Ingress",
        "Inspect an ingress event timeline",
        "Returns the ordered durable event ledger, including rejected, queued, claimed, and promoted events.",
        false,
        None,
        INGRESS_LOOKUP_PARAMS,
        200,
        "ingress event timeline",
        Example::IngressTimeline,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}",
        "Workflow Runs",
        "Get a workflow run",
        "Fetches a workflow run plus node-run records.",
        false,
        None,
        &[],
        200,
        "workflow run",
        Example::WorkflowRun,
    ),
    endpoint(
        "patch",
        "/workflow_runs/{id}",
        "Control Plane",
        "Update a workflow run",
        "Service-control endpoint used by runtime loops to update workflow-run status, state, and active node.",
        false,
        json_body("Workflow run status update.", Example::WorkflowRunStatus),
        &[],
        200,
        "workflow run updated",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/scheduler/workflow_runs/{id}/claim/renew",
        "Control Plane",
        "Renew a workflow-run claim",
        "Renews a scheduler lease for a claimed workflow run.",
        false,
        json_body(
            "Scheduler id and new lease deadline.",
            Example::SchedulerRunLease,
        ),
        &[],
        200,
        "workflow-run claim renewed",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/scheduler/workflow_runs/{id}/claim/release",
        "Control Plane",
        "Release a workflow-run claim",
        "Releases a scheduler lease for a claimed workflow run.",
        false,
        json_body(
            "Scheduler id releasing the claim.",
            Example::SchedulerRunLease,
        ),
        &[],
        200,
        "workflow-run claim released",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/cancel",
        "Workflow Runs",
        "Cancel a workflow run",
        "Requests cancellation for a workflow run and publishes the required runtime control signals.",
        false,
        None,
        &[],
        200,
        "workflow run cancel requested",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/pause",
        "Workflow Runs",
        "Pause a workflow run",
        "Requests that the engine pause a workflow run at a safe runtime boundary.",
        false,
        None,
        &[],
        200,
        "workflow run pause requested",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/resume",
        "Workflow Runs",
        "Resume a workflow run",
        "Requests that a paused workflow run resume execution.",
        false,
        None,
        &[],
        200,
        "workflow run resume requested",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/signals",
        "Workflow Runs",
        "Deliver a signal to a run",
        "Delivers an external signal payload to a parked node in one workflow run.",
        false,
        json_body("Signal name and payload.", Example::WebhookSignal),
        &[],
        200,
        "signal delivered",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/interrupts",
        "Workflow Runs",
        "Request an interrupt on a run",
        "Asks a run to raise an interrupt on its next drive, running the handler region declared \
         for that source. The request is refused and dropped when nothing can service it.",
        false,
        json_body(
            "Interrupt source, payload, and optional target cursor.",
            Example::InterruptRequest,
        ),
        &[],
        200,
        "interrupt requested",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/events/{node_id}",
        "Workflow Runs",
        "Deliver an event to a run",
        "Delivers an event to a parked event_source node, which consumes it and re-subscribes.",
        false,
        json_body("Event type and payload.", Example::EventDelivery),
        &[],
        200,
        "event delivered",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/replay",
        "Workflow Runs",
        "Replay a workflow run",
        "Creates a replay of a workflow run, optionally starting from a specific node id.",
        false,
        json_body("Optional replay start node.", Example::WorkflowRunReplay),
        &[],
        202,
        "workflow run replay accepted",
        Example::WorkflowRun,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/rename",
        "Workflow Runs",
        "Rename a workflow run",
        "Sets or clears the human-readable name of a workflow run.",
        false,
        json_body(
            "New workflow-run name; null clears it.",
            Example::WorkflowRunRename,
        ),
        &[],
        200,
        "workflow run renamed",
        Example::TaskResponse,
    ),
];

const INGRESS_LOOKUP_PARAMS: &[ParamDoc] = &[
    ParamDoc {
        name: "scope",
        location: "query",
        description: "Admission policy scope.",
        required: true,
        example: "release.lifecycle",
    },
    ParamDoc {
        name: "correlation_key",
        location: "query",
        description: "Provider-neutral correlation key.",
        required: true,
        example: "release-42",
    },
];
