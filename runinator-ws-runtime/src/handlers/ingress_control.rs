//! Durable operator control over external and broker ingress.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use chrono::Utc;
use runinator_engine::{
    audit::{AuditOutcome, record_audit},
    services::{IngressOperations, PipelineIngressRequest, PipelineOperations, RunOperations},
};
use runinator_models::{
    auth::{AuthContext, Permission},
    ingress_control::{
        BROKER_INGRESS_SESSION_TTL_SECONDS, BrokerIngressSession, BrokerIngressSessionMode,
        ExternalIngressGate, ExternalIngressGateMode, ExternalIngressRecord, IngressControlState,
    },
    orchestration::{IngressTarget, IngressTargetKind},
    rbac::{Action, ScopeKind, ScopeRef},
    replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance},
    validation::{Validate, ValidationError},
};
use runinator_store::roles::{DefinitionStore, DeliveryStore};
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::events::{AppEvent, AppEventKind, EventSender, emit};
use runinator_ws_core::models::{ApiResponse, IngressEventRequest};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthzChecker};
use serde::Deserialize;
use uuid::Uuid;

use super::runs::{RunOperationsStore, process_workflow_ingress};

pub trait IngressControlStore: RunOperationsStore + DeliveryStore + DefinitionStore {}

impl<T> IngressControlStore for T where T: RunOperationsStore + DeliveryStore + DefinitionStore {}

#[derive(Debug, Deserialize)]
pub struct GateRequest {
    mode: ExternalIngressGateMode,
}

#[derive(Debug, Deserialize)]
pub struct SessionRequest {
    scope: ScopeRef,
    mode: BrokerIngressSessionMode,
}

#[derive(Debug, Deserialize)]
pub struct SessionHeartbeatRequest {
    scope: ScopeRef,
}

impl Validate for GateRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

impl Validate for SessionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

impl Validate for SessionHeartbeatRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct ControlQuery {
    scope_kind: Option<ScopeKind>,
    scope_id: Option<Uuid>,
    target_kind: Option<IngressTargetKind>,
    target_id: Option<Uuid>,
    state: Option<IngressControlState>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ScopeQuery {
    scope_kind: ScopeKind,
    scope_id: Option<Uuid>,
}

fn target(kind: &str, id: Uuid) -> Result<IngressTarget, String> {
    let kind = match kind {
        "workflow" => IngressTargetKind::Workflow,
        "pipeline" => IngressTargetKind::Pipeline,
        _ => return Err("target kind must be workflow or pipeline".into()),
    };
    Ok(IngressTarget { kind, id })
}

fn scope(kind: ScopeKind, id: Option<Uuid>) -> Result<ScopeRef, String> {
    ScopeRef::new(kind, id).ok_or_else(|| {
        "platform scope must omit scope_id; organization/team/user scope must include it".into()
    })
}

async fn require_target<T: IngressControlStore>(
    db: &T,
    ctx: &AuthContext,
    target: &IngressTarget,
    permission: Permission,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    let checker = AuthzChecker::new(db, ctx);
    match target.kind {
        IngressTargetKind::Workflow => checker.require_workflow(target.id, permission).await,
        IngressTargetKind::Pipeline => checker.require_pipeline(target.id, permission).await,
    }
}

#[allow(clippy::result_large_err)]
fn require_broker_scope(
    ctx: &AuthContext,
    scope: ScopeRef,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    if ctx.is_platform_admin() {
        Ok(())
    } else {
        ctx.require_scope_action(Action::EngineOperate, scope)
    }
}

fn emit_change(
    events: &EventSender,
    stream: &str,
    record_id: Uuid,
    state: &str,
    owner_scope: ScopeRef,
) {
    emit(
        events,
        AppEvent::for_scope(
            owner_scope,
            AppEventKind::IngressControlChanged {
                stream: stream.into(),
                record_id,
                state: state.into(),
                owner_scope,
            },
        ),
    );
}

pub async fn get_gate<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> (StatusCode, Json<ApiResponse>) {
    let target = match target(&kind, id) {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
    if let Err(reply) = require_target(db.as_ref(), &ctx, &target, Permission::View).await {
        return reply;
    }
    let ingress = IngressOperations::new(db);
    match ingress.gate(target).await {
        Ok(Some(gate)) => (StatusCode::OK, Json(ApiResponse::ExternalIngressGate(gate))),
        Ok(None) => not_found("ingress gate not configured"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn put_gate<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Extension(events): Extension<EventSender>,
    Path((kind, id)): Path<(String, Uuid)>,
    ValidatedJson(request): ValidatedJson<GateRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let target = match target(&kind, id) {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
    if let Err(reply) = require_target(db.as_ref(), &ctx, &target, Permission::Edit).await {
        return reply;
    }
    let ingress = IngressOperations::new(db.clone());
    let owner_scope = match ingress.owner_scope_for_target(&target).await {
        Ok(value) => value,
        Err(error) => return api_error(error.to_string()),
    };
    match ingress
        .set_gate(ExternalIngressGate {
            target: target.clone(),
            owner_scope,
            mode: request.mode,
            updated_by: ctx.principal_id,
            updated_at: Utc::now(),
        })
        .await
    {
        Ok(gate) => {
            emit_change(
                &events,
                "external",
                target.id,
                gate.mode.as_str(),
                owner_scope,
            );
            record_audit(
                db.as_ref(),
                ctx.principal_id,
                ctx.actor_kind(),
                "ingress.gate.change",
                AuditOutcome::Success,
                Some(match target.kind {
                    IngressTargetKind::Workflow => "workflow",
                    IngressTargetKind::Pipeline => "pipeline",
                }),
                Some(target.id),
                Some("external ingress gate changed"),
            )
            .await;
            (StatusCode::OK, Json(ApiResponse::ExternalIngressGate(gate)))
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn list_external<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<ControlQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let scope = match query.scope_kind {
        Some(kind) => match scope(kind, query.scope_id) {
            Ok(value) => Some(value),
            Err(error) => return bad_request(error),
        },
        None if query.scope_id.is_some() => {
            return bad_request("scope_kind is required with scope_id");
        }
        None => None,
    };
    let target = match (query.target_kind, query.target_id) {
        (Some(kind), Some(id)) => Some(IngressTarget { kind, id }),
        (None, None) => None,
        _ => return bad_request("target_kind and target_id must be supplied together"),
    };
    let ingress = IngressOperations::new(db.clone());
    let records = match ingress
        .review_records(
            scope,
            target,
            query.state,
            query.limit.unwrap_or(200).clamp(1, 1000),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => return api_error(error.to_string()),
    };
    let mut visible = Vec::new();
    for record in records {
        if require_target(db.as_ref(), &ctx, &record.target, Permission::View)
            .await
            .is_ok()
        {
            visible.push(record);
        }
    }
    (
        StatusCode::OK,
        Json(ApiResponse::ExternalIngressRecordList(visible)),
    )
}

pub async fn get_external<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let ingress = IngressOperations::new(db.clone());
    let record = match ingress.review_record(id).await {
        Ok(Some(value)) => value,
        Ok(None) => return not_found("external ingress record not found"),
        Err(error) => return api_error(error.to_string()),
    };
    if let Err(reply) = require_target(db.as_ref(), &ctx, &record.target, Permission::View).await {
        return reply;
    }
    (
        StatusCode::OK,
        Json(ApiResponse::ExternalIngressRecord(record)),
    )
}

async fn apply_external<T: IngressControlStore>(
    db: Arc<T>,
    runs: Arc<RunOperations<T>>,
    pipelines: Arc<PipelineOperations<T>>,
    ctx: &AuthContext,
    events: &EventSender,
    record: ExternalIngressRecord,
) -> ExternalIngressRecord {
    let ingress = IngressOperations::new(db.clone());
    let result = match record.target.kind {
        IngressTargetKind::Workflow => {
            let request = IngressEventRequest {
                source: record.event.source.clone(),
                event_id: record.event.event_id.clone(),
                event_type: record.event.event_type.clone(),
                correlation_key: record.event.correlation_key.clone(),
                payload: record.event.payload.clone(),
                provenance: record.event.provenance.clone(),
                occurred_at: record.event.occurred_at,
            };
            let provenance = WorkflowRunProvenance {
                source_kind: Some(TriggerSourceKind::Api),
                actor_type: Some(TriggerActorType::User),
                actor_replica_id: None,
                actor_display_name: Some("ingress-control".into()),
                request_host: None,
                request_ip: None,
                metadata: record.event.provenance.clone(),
            };
            let (status, _) = process_workflow_ingress(
                db.clone(),
                runs,
                ctx.org_id,
                ctx.principal_id,
                record.target.id,
                request,
                provenance,
                true,
            )
            .await;
            if status.is_success() {
                Ok(())
            } else {
                Err(format!("workflow ingress returned {status}"))
            }
        }
        IngressTargetKind::Pipeline => pipelines
            .process_approved_ingress(
                record.target.id,
                ctx.org_id,
                PipelineIngressRequest {
                    source: record.event.source.clone(),
                    event_id: record.event.event_id.clone(),
                    event_type: record.event.event_type.clone(),
                    correlation_key: record.event.correlation_key.clone(),
                    payload: record.event.payload.clone(),
                    provenance: record.event.provenance.clone(),
                    occurred_at: record.event.occurred_at,
                },
                None,
            )
            .await
            .map(|_| ())
            .map_err(|error| format!("{error:?}")),
    };
    let (state, error) = match result {
        Ok(()) => (IngressControlState::Applied, None),
        Err(error) => (IngressControlState::Failed, Some(error)),
    };
    let _ = ingress.finish_review(record.id, state, error).await;
    emit_change(
        events,
        "external",
        record.id,
        match state {
            IngressControlState::Applied => "applied",
            IngressControlState::Failed => "failed",
            _ => "applying",
        },
        record.owner_scope,
    );
    ingress
        .review_record(record.id)
        .await
        .ok()
        .flatten()
        .unwrap_or(record)
}

pub async fn approve_external<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(runs): Extension<Arc<RunOperations<T>>>,
    Extension(pipelines): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Extension(events): Extension<EventSender>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let ingress = IngressOperations::new(db.clone());
    let pending = match ingress.review_record(id).await {
        Ok(Some(value)) => value,
        Ok(None) => return not_found("external ingress record not found"),
        Err(error) => return api_error(error.to_string()),
    };
    if let Err(reply) = require_target(db.as_ref(), &ctx, &pending.target, Permission::Run).await {
        return reply;
    }
    if pending.gate_mode != ExternalIngressGateMode::Review {
        return bad_request("paused queues must be released in FIFO order");
    }
    let actor = ctx.principal_id.unwrap_or(Uuid::nil());
    let claimed = match ingress.claim_review(id, actor).await {
        Ok(Some(value)) => value,
        Ok(None) => return bad_request("external ingress record is no longer held"),
        Err(error) => return api_error(error.to_string()),
    };
    let record = apply_external(db.clone(), runs, pipelines, &ctx, &events, claimed).await;
    record_audit(
        db.as_ref(),
        ctx.principal_id,
        ctx.actor_kind(),
        "ingress.external.approve",
        AuditOutcome::Success,
        Some("ingress_event"),
        Some(id),
        None,
    )
    .await;
    (
        StatusCode::OK,
        Json(ApiResponse::ExternalIngressRecord(record)),
    )
}

pub async fn release_external<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(runs): Extension<Arc<RunOperations<T>>>,
    Extension(pipelines): Extension<Arc<PipelineOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Extension(events): Extension<EventSender>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> (StatusCode, Json<ApiResponse>) {
    let target = match target(&kind, id) {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
    if let Err(reply) = require_target(db.as_ref(), &ctx, &target, Permission::Run).await {
        return reply;
    }
    let ingress = IngressOperations::new(db.clone());
    if !matches!(
        ingress.gate(target.clone()).await,
        Ok(Some(ExternalIngressGate {
            mode: ExternalIngressGateMode::Paused,
            ..
        }))
    ) {
        return bad_request("FIFO release is available only while the target gate is paused");
    }
    let actor = ctx.principal_id.unwrap_or(Uuid::nil());
    let mut released = Vec::new();
    loop {
        let Some(record) = (match ingress.claim_oldest_review(target.clone(), actor).await {
            Ok(value) => value,
            Err(error) => return api_error(error.to_string()),
        }) else {
            break;
        };
        released.push(
            apply_external(
                db.clone(),
                runs.clone(),
                pipelines.clone(),
                &ctx,
                &events,
                record,
            )
            .await,
        );
    }
    record_audit(
        db.as_ref(),
        ctx.principal_id,
        ctx.actor_kind(),
        "ingress.external.release",
        AuditOutcome::Success,
        Some(match target.kind {
            IngressTargetKind::Workflow => "workflow",
            IngressTargetKind::Pipeline => "pipeline",
        }),
        Some(target.id),
        Some(&format!("released {} events", released.len())),
    )
    .await;
    (
        StatusCode::OK,
        Json(ApiResponse::ExternalIngressRecordList(released)),
    )
}

pub async fn drop_external<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Extension(events): Extension<EventSender>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let ingress = IngressOperations::new(db.clone());
    let record = match ingress.review_record(id).await {
        Ok(Some(value)) => value,
        Ok(None) => return not_found("external ingress record not found"),
        Err(error) => return api_error(error.to_string()),
    };
    if let Err(reply) = require_target(db.as_ref(), &ctx, &record.target, Permission::Run).await {
        return reply;
    }
    if !matches!(
        ingress
            .drop_review(id, ctx.principal_id.unwrap_or(Uuid::nil()))
            .await,
        Ok(true)
    ) {
        return bad_request("external ingress record is no longer held");
    }
    emit_change(&events, "external", id, "dropped", record.owner_scope);
    record_audit(
        db.as_ref(),
        ctx.principal_id,
        ctx.actor_kind(),
        "ingress.external.drop",
        AuditOutcome::Success,
        Some("ingress_event"),
        Some(id),
        None,
    )
    .await;
    get_external(Extension(db), Extension(ctx), Path(id)).await
}

pub async fn put_broker_session<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Extension(events): Extension<EventSender>,
    ValidatedJson(request): ValidatedJson<SessionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_broker_scope(&ctx, request.scope) {
        return reply;
    }
    let ingress = IngressOperations::new(db.clone());
    let session = BrokerIngressSession {
        scope: request.scope,
        mode: request.mode,
        updated_by: ctx.principal_id,
        updated_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::seconds(BROKER_INGRESS_SESSION_TTL_SECONDS),
    };
    match ingress.set_broker_session(session).await {
        Ok(session) => {
            emit_change(
                &events,
                "broker",
                request.scope.id.unwrap_or(Uuid::nil()),
                request.mode.as_str(),
                request.scope,
            );
            record_audit(
                db.as_ref(),
                ctx.principal_id,
                ctx.actor_kind(),
                "ingress.broker.session",
                AuditOutcome::Success,
                Some("scope"),
                request.scope.id,
                Some(request.scope.kind.as_str()),
            )
            .await;
            (
                StatusCode::OK,
                Json(ApiResponse::BrokerIngressSession(session)),
            )
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn get_broker_session<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<ScopeQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let scope = match scope(query.scope_kind, query.scope_id) {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
    if let Err(reply) = require_broker_scope(&ctx, scope) {
        return reply;
    }
    let ingress = IngressOperations::new(db);
    match ingress.broker_session(scope).await {
        Ok(Some(value)) => (
            StatusCode::OK,
            Json(ApiResponse::BrokerIngressSession(value)),
        ),
        Ok(None) => not_found("broker ingress session not configured"),
        Err(error) => api_error(error.to_string()),
    }
}

/// Renew the short-lived inspector lease without creating an audit entry for every browser
/// heartbeat. Once these renewals stop, the engine treats the session as off.
pub async fn heartbeat_broker_session<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<SessionHeartbeatRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_broker_scope(&ctx, request.scope) {
        return reply;
    }
    let ingress = IngressOperations::new(db.clone());
    let current = match ingress.broker_session(request.scope).await {
        Ok(Some(session)) => session,
        Ok(None) => return not_found("broker ingress session is no longer active"),
        Err(error) => return api_error(error.to_string()),
    };
    let now = Utc::now();
    let session = BrokerIngressSession {
        scope: current.scope,
        mode: current.mode,
        updated_by: ctx.principal_id,
        updated_at: now,
        expires_at: now + chrono::Duration::seconds(BROKER_INGRESS_SESSION_TTL_SECONDS),
    };
    match ingress.set_broker_session(session).await {
        Ok(session) => (
            StatusCode::OK,
            Json(ApiResponse::BrokerIngressSession(session)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn list_broker<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<ControlQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let ingress = IngressOperations::new(db.clone());
    let scope = match query.scope_kind {
        Some(kind) => match scope(kind, query.scope_id) {
            Ok(value) => Some(value),
            Err(error) => return bad_request(error),
        },
        None if query.scope_id.is_some() => {
            return bad_request("scope_kind is required with scope_id");
        }
        None => None,
    };
    let Some(scope) = scope else {
        if !ctx.is_platform_admin() {
            return bad_request("non-admin callers must select an exact broker scope");
        } else {
            return match ingress
                .broker_records(None, query.state, query.limit.unwrap_or(200).clamp(1, 1000))
                .await
            {
                Ok(value) => (
                    StatusCode::OK,
                    Json(ApiResponse::BrokerIngressRecordList(value)),
                ),
                Err(error) => api_error(error.to_string()),
            };
        }
    };
    if let Err(reply) = require_broker_scope(&ctx, scope) {
        return reply;
    }
    match ingress
        .broker_records(
            Some(scope),
            query.state,
            query.limit.unwrap_or(200).clamp(1, 1000),
        )
        .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::BrokerIngressRecordList(value)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

async fn decide_broker<T: IngressControlStore>(
    db: Arc<T>,
    ctx: AuthContext,
    events: EventSender,
    id: Uuid,
    state: IngressControlState,
) -> (StatusCode, Json<ApiResponse>) {
    let ingress = IngressOperations::new(db.clone());
    let record = match ingress.broker_record(id).await {
        Ok(Some(value)) => value,
        Ok(None) => return not_found("broker ingress record not found"),
        Err(error) => return api_error(error.to_string()),
    };
    if let Err(reply) = require_broker_scope(&ctx, record.scope) {
        return reply;
    }
    match ingress
        .decide_broker_record(id, state, ctx.principal_id.unwrap_or(Uuid::nil()))
        .await
    {
        Ok(true) => {
            emit_change(
                &events,
                "broker",
                id,
                if state == IngressControlState::Approved {
                    "approved"
                } else {
                    "dropped"
                },
                record.scope,
            );
            let action = if state == IngressControlState::Approved {
                "ingress.broker.approve"
            } else {
                "ingress.broker.drop"
            };
            record_audit(
                db.as_ref(),
                ctx.principal_id,
                ctx.actor_kind(),
                action,
                AuditOutcome::Success,
                Some("broker_ingress"),
                Some(id),
                None,
            )
            .await;
            match ingress.broker_record(id).await {
                Ok(Some(value)) => (
                    StatusCode::OK,
                    Json(ApiResponse::BrokerIngressRecord(value)),
                ),
                Ok(None) => not_found("broker ingress record not found"),
                Err(error) => api_error(error.to_string()),
            }
        }
        Ok(false) => bad_request("broker ingress record is no longer held"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn approve_broker<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Extension(events): Extension<EventSender>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    decide_broker(db, ctx, events, id, IngressControlState::Approved).await
}
pub async fn drop_broker<T: IngressControlStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Extension(events): Extension<EventSender>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    decide_broker(db, ctx, events, id, IngressControlState::Dropped).await
}

pub fn routes<T: IngressControlStore>(pool: Arc<T>) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            "/ingress_control/targets/{kind}/{id}/gate",
            get(get_gate::<T>).put(put_gate::<T>),
        )
        .route(
            "/ingress_control/targets/{kind}/{id}/release",
            post(release_external::<T>),
        )
        .route("/ingress_control/external", get(list_external::<T>))
        .route("/ingress_control/external/{id}", get(get_external::<T>))
        .route(
            "/ingress_control/external/{id}/approve",
            post(approve_external::<T>),
        )
        .route(
            "/ingress_control/external/{id}/drop",
            post(drop_external::<T>),
        )
        .route(
            "/ingress_control/broker/session",
            get(get_broker_session::<T>).put(put_broker_session::<T>),
        )
        .route(
            "/ingress_control/broker/session/heartbeat",
            post(heartbeat_broker_session::<T>),
        )
        .route("/ingress_control/broker", get(list_broker::<T>))
        .route(
            "/ingress_control/broker/{id}/approve",
            post(approve_broker::<T>),
        )
        .route("/ingress_control/broker/{id}/drop", post(drop_broker::<T>))
        .layer(Extension(pool))
}
