//! replica-scoped durable agent directives.

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use chrono::{Duration, Utc};
use runinator_comm::AgentDirectiveKind;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::AuthContext,
    rbac::{Action, ScopeRef},
};
use runinator_ws_core::{
    events::{AppEvent, AppEventKind, EventSender, emit, nudge_agent_directive_publisher},
    models::{AgentDirectiveQuery, ApiResponse, CreateAgentDirectiveRequest},
    openapi::docs::{EndpointDoc, Example, endpoint, json_body},
    responses::{api_error, not_found},
};
use runinator_ws_middleware::authz::AuthContextExt;
use uuid::Uuid;

use crate::repository;

pub async fn create<T: DatabaseImpl>(
    Extension(db): Extension<std::sync::Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(replica_id): Path<Uuid>,
    Json(request): Json<CreateAgentDirectiveRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let (action, scope) = required_policy(&ctx, &request.kind);
    if let Err(reply) = ctx.require_scope_action(action, scope) {
        return reply;
    }
    match repository::fetch_replica(db.as_ref(), replica_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(format!("Replica {replica_id} not found")),
        Err(err) => return api_error(err.to_string()),
    }
    let ttl = request.expires_in_seconds.unwrap_or(300).clamp(1, 86_400);
    match repository::enqueue_agent_directive(
        db.as_ref(),
        replica_id,
        request.kind,
        Utc::now() + Duration::seconds(ttl as i64),
    )
    .await
    {
        Ok(record) => {
            nudge_agent_directive_publisher(&events);
            emit(&events, AppEvent::global(AppEventKind::ReplicasChanged));
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::AgentDirective(record)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list<T: DatabaseImpl>(
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
    match repository::list_agent_directives(db.as_ref(), replica_id, query.limit.unwrap_or(100))
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
        AgentDirectiveKind::SetLabels { .. }
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

pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
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
