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
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, RbacStore, ReplicaStore},
};
use runinator_ws_core::{
    ValidatedJson,
    events::{AppEvent, AppEventKind, EventSender, emit, nudge_agent_directives},
    models::{AgentDirectiveQuery, ApiResponse, CreateAgentDirectiveRequest},
    openapi::docs::{EndpointDoc, Example, endpoint, json_body},
    responses::{api_error, not_found, task_response_success},
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

pub async fn list_machines<T: AuthStore + RbacStore>(
    Extension(db): Extension<std::sync::Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::AgentsEnroll, ScopeRef::PLATFORM) {
        return reply;
    }
    match ReplicaRegistry::new(db).agent_machines().await {
        Ok(machines) => match machines
            .into_iter()
            .map(|machine| serde_json::to_value(machine).map(runinator_models::value::Value::from))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(err) => api_error(err.to_string()),
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn invalidate_machine<T: AuthStore + RbacStore + ReplicaStore + RuntimeStore>(
    Extension(db): Extension<std::sync::Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(machine_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::AgentsEnroll, ScopeRef::PLATFORM) {
        return reply;
    }
    match ReplicaRegistry::new(db)
        .invalidate_machine(machine_id, &ctx)
        .await
    {
        Ok(Some(result)) => {
            emit(&events, AppEvent::global(AppEventKind::ReplicasChanged));
            task_response_success(format!(
                "Machine invalidated; revoked {} credential(s) and kicked {} replica(s)",
                result.revoked_credentials, result.kicked_replicas
            ))
        }
        Ok(None) => not_found("Enrolled machine not found"),
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

pub fn routes<T: ReplicaStore + AuthStore + RbacStore + RuntimeStore>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::routing::{delete, get};
    axum::Router::new()
        .route(
            "/replicas/{replica_id}/directives",
            get(list::<T>)
                .post(create::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/agents/machines",
            get(list_machines::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/agents/machines/{machine_id}",
            delete(invalidate_machine::<T>).layer(Extension(pool)),
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
    endpoint(
        "get",
        "/agents/machines",
        "Agents",
        "List enrolled machines",
        "Lists timed and permanent agent machine enrollments and their current credential state.",
        false,
        None,
        &[],
        200,
        "enrolled machines",
        Example::None,
    ),
    endpoint(
        "delete",
        "/agents/machines/{machine_id}",
        "Agents",
        "Invalidate an enrolled machine",
        "Disables the machine principal, revokes every agent credential it owns, and kicks its current replicas.",
        false,
        None,
        &[],
        200,
        "machine invalidated",
        Example::TaskResponse,
    ),
];
