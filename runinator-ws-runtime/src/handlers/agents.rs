//! replica-scoped durable agent directives.

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use chrono::{Duration, Utc};
use runinator_comm::AgentDirectiveKind;
use runinator_engine::services::ReplicaRegistry;
use runinator_models::{
    auth::AuthContext,
    rbac::{Action, ScopeRef},
};
use runinator_store::roles::ReplicaStore;
use runinator_ws_core::{
    ValidatedJson,
    events::{AppEvent, AppEventKind, EventSender, emit, nudge_agent_directives},
    models::{AgentDirectiveQuery, ApiResponse, CreateAgentDirectiveRequest},
    openapi::docs::{EndpointDoc, Example, endpoint, json_body},
    responses::{api_error, not_found},
};
use runinator_ws_middleware::authz::AuthContextExt;
use uuid::Uuid;

pub async fn create<T: ReplicaStore>(
    Extension(db): Extension<std::sync::Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<CreateAgentDirectiveRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let (action, scope) = required_policy(&ctx, &request.kind);
    if let Err(reply) = ctx.require_scope_action(action, scope) {
        return reply;
    }
    let ttl = request.expires_in_seconds.unwrap_or(300).clamp(1, 86_400);
    match ReplicaRegistry::new(db)
        .issue_directive(
            replica_id,
            request.kind,
            Utc::now() + Duration::seconds(ttl as i64),
        )
        .await
    {
        Ok(Some(record)) => {
            // The durable outbox is authoritative; these optional hints only reduce local delivery
            // and WebSocket-update latency when this replica embeds the engine.
            nudge_agent_directives(&events);
            emit(&events, AppEvent::global(AppEventKind::ReplicasChanged));
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::AgentDirective(record)),
            )
        }
        Ok(None) => not_found(format!("Replica {replica_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list<T: ReplicaStore>(
    Extension(db): Extension<std::sync::Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    Query(query): Query<AgentDirectiveQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::AuditRead,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    let can_read_files = ctx
        .require_scope_action(
            runinator_models::rbac::Action::SecretsRead,
            ctx.selected_scope(),
        )
        .is_ok();
    match ReplicaRegistry::new(db)
        .directives(replica_id, query.limit.unwrap_or(100))
        .await
    {
        Ok(mut records) => {
            if !can_read_files {
                for record in &mut records {
                    if matches!(record.kind, AgentDirectiveKind::FetchFile { .. }) {
                        record.payload = runinator_models::value::Value::Null;
                        record.message = Some("file result requires secrets:read".to_string());
                    }
                }
            }
            (
                StatusCode::OK,
                Json(ApiResponse::AgentDirectiveList(records)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

fn required_policy(ctx: &AuthContext, kind: &AgentDirectiveKind) -> (Action, ScopeRef) {
    match kind {
        AgentDirectiveKind::Diagnostics
        | AgentDirectiveKind::TailLogs { .. }
        | AgentDirectiveKind::ListSandbox { .. } => (Action::AuditRead, ScopeRef::PLATFORM),
        AgentDirectiveKind::FetchFile { .. } => (Action::SecretsRead, ctx.selected_scope()),
        AgentDirectiveKind::CleanupWorkspace { .. }
        | AgentDirectiveKind::SetLabels { .. }
        | AgentDirectiveKind::SetConcurrency { .. }
        | AgentDirectiveKind::SetLogLevel { .. }
        | AgentDirectiveKind::RepublishProviders
        | AgentDirectiveKind::Drain
        | AgentDirectiveKind::Undrain
        | AgentDirectiveKind::Restart
        | AgentDirectiveKind::RotateCredential
        | AgentDirectiveKind::Unknown => (Action::NodesOperate, ScopeRef::PLATFORM),
    }
}

pub fn routes<T: ReplicaStore>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::routing::get;
    axum::Router::new().route(
        "/replicas/{replica_id}/directives",
        get(list::<T>).post(create::<T>).layer(Extension(pool)),
    )
}

pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "post",
        "/replicas/{replica_id}/directives",
        "Agents",
        "Issue an agent directive",
        "Durably queues a constrained management operation for one replica.",
        false,
        json_body(
            "Tagged directive and optional expiry.",
            Example::AgentDirective,
        ),
        &[],
        202,
        "directive queued",
        Example::AgentDirective,
    ),
    endpoint(
        "get",
        "/replicas/{replica_id}/directives",
        "Agents",
        "List agent directives",
        "Lists recent directives and their result state for one replica.",
        false,
        None,
        &[],
        200,
        "directives",
        Example::AgentDirectiveList,
    ),
];
