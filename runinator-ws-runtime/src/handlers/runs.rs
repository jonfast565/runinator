use std::{net::SocketAddr, sync::Arc};
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_broker_core::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance};
use runinator_models::runs::NewRunChunk;
use serde::Deserialize;

use crate::repository;
use runinator_ws_core::events::{
    AppEvent, AppEventKind, EventSender, emit, emit_task_run, emit_workflow_run,
    nudge_wake_publisher,
};
use runinator_ws_core::models::{
    self, ApiResponse, RunStatusQuery, RunStatusRequest, SchedulerRunClaimReleaseRequest,
    SchedulerRunClaimRenewRequest, SchedulerRunClaimRequest, TaskResponseSchema,
    WorkflowRunRequest, WorkflowRunStatusQuery, WorkflowRunStatusRequest,
    WorkflowTriggerRunRequest,
};
use runinator_ws_core::openapi::docs::{
    CURSOR, EndpointDoc, Example, RUN_FILTERS, WORKFLOW_RUN_FILTERS, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::AuthContextExt;
use runinator_ws_middleware::authz::AuthzChecker;

#[derive(Debug, Default, Deserialize)]
pub struct ChunkQuery {
    pub cursor: Option<i64>,
    pub limit: Option<i64>,
}

pub async fn create_workflow_trigger_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    _headers: HeaderMap,
    _connect: ConnectInfo<SocketAddr>,
    Path(trigger_id): Path<Uuid>,
    Json(request): Json<WorkflowTriggerRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_trigger_workflow(trigger_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match repository::create_workflow_run_for_trigger(
        db.as_ref(),
        trigger_id,
        request.parameters,
        request.debug,
        None,
        Some(request_actor_display_name()),
    )
    .await
    {
        Ok(run) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), run.id).await;
            emit_workflow_run(&events, run.id, org_id);
            nudge_wake_publisher(&events);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
                    run,
                    Vec::new(),
                ))),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    headers: HeaderMap,
    ConnectInfo(connect): ConnectInfo<SocketAddr>,
    Path(workflow_id): Path<Uuid>,
    Json(request): Json<WorkflowRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match repository::create_workflow_run(
        db.as_ref(),
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
    )
    .await
    {
        Ok(run) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), run.id).await;
            emit_workflow_run(&events, run.id, org_id);
            nudge_wake_publisher(&events);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
                    run,
                    Vec::new(),
                ))),
            )
        }
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

pub async fn claim_workflow_runs_for_scheduler<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Json(request): Json<SchedulerRunClaimRequest>,
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
            runinator_models::workflows::WorkflowStatus::ApprovalRequired,
            runinator_models::workflows::WorkflowStatus::InputRequired,
            runinator_models::workflows::WorkflowStatus::Blocked,
        ]
    } else {
        request.statuses
    };
    match repository::claim_workflow_runs_for_scheduler(
        db.as_ref(),
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

pub async fn renew_workflow_run_claim<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(request): Json<SchedulerRunClaimRenewRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::renew_workflow_run_claim(
        db.as_ref(),
        workflow_run_id,
        request.scheduler_id,
        request.lease_until,
    )
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

pub async fn release_workflow_run_claim<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(request): Json<SchedulerRunClaimReleaseRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::release_workflow_run_claim(db.as_ref(), workflow_run_id, request.scheduler_id)
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
    responses(
        (status = 200, description = "workflow run cancel requested", body = TaskResponseSchema),
        (status = 400, description = "workflow run could not be canceled", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn cancel_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(broker): Extension<Arc<dyn Broker>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match repository::cancel_workflow_run(db.as_ref(), broker.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            nudge_wake_publisher(&events);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/workflow_runs/{id}/pause",
    tag = "Workflow Runs",
    params(("id" = Uuid, Path, description = "Workflow run identifier.")),
    responses(
        (status = 200, description = "workflow run pause requested", body = TaskResponseSchema),
        (status = 400, description = "workflow run could not be paused", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn pause_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match repository::pause_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/workflow_runs/{id}/resume",
    tag = "Workflow Runs",
    params(("id" = Uuid, Path, description = "Workflow run identifier.")),
    responses(
        (status = 200, description = "workflow run resume requested", body = TaskResponseSchema),
        (status = 400, description = "workflow run could not be resumed", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn resume_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match repository::resume_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
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
pub async fn replay_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    body: Option<Json<runinator_ws_core::models::WorkflowRunReplayRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    let from_step_id = body.and_then(|Json(request)| request.from_step_id);
    match repository::replay_workflow_run(db.as_ref(), workflow_run_id, from_step_id).await {
        Ok(run) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), run.id).await;
            emit_workflow_run(&events, run.id, org_id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
                    run,
                    Vec::new(),
                ))),
            )
        }
        Err(err) => bad_request(err.to_string()),
    }
}

/// deliver an event to a parked `event_source` node in one run. the node consumes it on the next
/// drive and re-parks, so repeated deliveries drive repeated iterations of its body.
pub async fn deliver_run_event<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path((workflow_run_id, node_id)): Path<(Uuid, String)>,
    Json(request): Json<runinator_ws_core::models::EventDeliveryRequest>,
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
    match repository::deliver_run_event(db.as_ref(), workflow_run_id, node_id, event).await {
        Ok(response) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(response)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub async fn deliver_signal<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(request): Json<runinator_ws_core::models::SignalDeliveryRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Run)
        .await
    {
        return reply;
    }
    match repository::deliver_signal(db.as_ref(), workflow_run_id, request.name, request.payload)
        .await
    {
        Ok(response) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(response)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

/// ask a run to raise an interrupt, running the handler region declared for that source.
///
/// nothing about serviceability is decided here — the request is recorded on the thread and the VM
/// raises or drops it on the next drive of the target thread. that keeps one copy of the fail-open
/// rules, in the crate that owns them.
pub async fn request_interrupt<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(request): Json<runinator_ws_core::models::InterruptRequest>,
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
    match repository::request_run_interrupt(
        db.as_ref(),
        workflow_run_id,
        source,
        request.payload,
        request.continuation_id,
    )
    .await
    {
        Ok(response) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(response)))
        }
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
pub async fn rename_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(request): Json<runinator_ws_core::models::WorkflowRunRenameRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Edit)
        .await
    {
        return reply;
    }
    match repository::set_workflow_run_name(db.as_ref(), workflow_run_id, request.name).await {
        Ok(response) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(response)))
        }
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
pub async fn get_workflow_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
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
        return match repository::fetch_workflow_runs_by_name(
            db.as_ref(),
            name,
            query.open.unwrap_or(false),
        )
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
        return match repository::fetch_workflow_runs_for_workflow(db.as_ref(), workflow_id).await {
            Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
            Err(err) => api_error(err.to_string()),
        };
    }

    if let Some(status) = query.status {
        return match repository::fetch_workflow_runs_by_status(db.as_ref(), status).await {
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
    match repository::fetch_recent_workflow_runs(db.as_ref(), limit).await {
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

pub async fn get_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Query(query): Query<RunStatusQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    let Some(status) = query.status else {
        return bad_request("run status query is required");
    };
    match repository::fetch_runs_by_status(db.as_ref(), status).await {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse::RunList(runs))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(run_id): Path<Uuid>,
    Json(request): Json<RunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::update_run_status(
        db.as_ref(),
        run_id,
        request.status,
        request.output_json,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            emit_task_run(&events, run_id, request.status, ctx.org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_run_chunks<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(run_id): Path<Uuid>,
    Query(query): Query<ChunkQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::fetch_run_chunks(
        db.as_ref(),
        run_id,
        query.cursor,
        query.limit.unwrap_or(100),
    )
    .await
    {
        Ok(chunks) => (StatusCode::OK, Json(ApiResponse::RunChunks(chunks))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn append_run_chunk<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(run_id): Path<Uuid>,
    Json(chunk): Json<NewRunChunk>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::append_run_chunk(db.as_ref(), run_id, &chunk).await {
        Ok(chunk) => {
            emit(
                &events,
                AppEvent::new(ctx.org_id, AppEventKind::RunChunkAdded { run_id }),
            );
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::RunChunks(vec![chunk])),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(request): Json<WorkflowRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_system_role(&[
        runinator_models::rbac::SystemRole::Engine,
        runinator_models::rbac::SystemRole::Worker,
        runinator_models::rbac::SystemRole::Agent,
    ]) {
        return reply;
    }
    match repository::update_workflow_run_status(
        db.as_ref(),
        workflow_run_id,
        request.status,
        request.active_node_id,
        None,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::View)
        .await
    {
        return reply;
    }
    match repository::fetch_workflow_run(db.as_ref(), workflow_run_id).await {
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

pub async fn delete_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(workflow_run_id, runinator_models::auth::Permission::Edit)
        .await
    {
        return reply;
    }
    match repository::delete_workflow_run(db.as_ref(), workflow_run_id).await {
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
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, patch, post};
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
            runinator_models::api_routes::API_RUNS,
            get(get_runs::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/runs/{id}",
            patch(update_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/runs/{id}/chunks",
            get(get_run_chunks::<T>)
                .post(append_run_chunk::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/runs",
            post(create_workflow_run::<T>).layer(Extension(pool.clone())),
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
        "get",
        "/runs",
        "Runs",
        "List task runs by status",
        "Service-control endpoint that lists low-level task runs for a required status.",
        false,
        None,
        RUN_FILTERS,
        200,
        "task runs",
        Example::RunList,
    ),
    endpoint(
        "patch",
        "/runs/{id}",
        "Runs",
        "Update a task run",
        "Service-control endpoint used by workers to update low-level task-run status and output.",
        false,
        json_body("Task run status update.", Example::RunStatus),
        &[],
        200,
        "task run updated",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/runs/{id}/chunks",
        "Runs",
        "List task run chunks",
        "Service-control endpoint that returns streamed chunks for a low-level task run.",
        false,
        None,
        CURSOR,
        200,
        "task run chunks",
        Example::RunChunk,
    ),
    endpoint(
        "post",
        "/runs/{id}/chunks",
        "Runs",
        "Append a task run chunk",
        "Service-control endpoint used by workers to append stdout, stderr, log, or structured chunks.",
        false,
        json_body("Run chunk to append.", Example::RunChunk),
        &[],
        202,
        "task run chunk appended",
        Example::RunChunk,
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
