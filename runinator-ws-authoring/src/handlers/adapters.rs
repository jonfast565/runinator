//! Org-scoped orchestration adapter definitions and adapter-host diagnostics.

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Extension, Json,
    body::Bytes,
    extract::Path,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use chrono::Utc;
use runinator_adapter_contract::{AdapterPollRequest, AdapterPollResponse, AdapterRequest};
use runinator_broker_core::{UiEventPublisher, emit_adapter};
use runinator_engine::services::{AdapterOperations, PipelineOperations};
use runinator_models::{
    auth::{AuthContext, Permission, PrincipalKind, ResourceType},
    orchestration::{AdapterDefinition, AdapterKindMetadata, AdapterTransport},
    rbac::{Action, ScopeKind, ScopeRef},
    web::TaskResponse,
};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, IngressStore, NewAdapterDefinition, NewAdapterRevision,
        OrchestrationStore, RbacStore, ScheduleStore, SettingStore, WorkflowVmStore,
    },
};
use runinator_ws_core::{
    ValidatedJson,
    models::{
        AdapterApplyRequest, AdapterEnableRequest, AdapterTestRequest, ApiResponse,
        IngressEventRequest,
    },
    openapi::docs::{EndpointDoc, EndpointPolicy, Example, endpoint_with_policy, json_body},
    responses::{api_error, bad_request, not_found},
};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker};
use uuid::Uuid;

use super::pipelines::process_pipeline_ingress;

async fn catalog() -> Result<Vec<runinator_models::orchestration::AdapterKindCatalogEntry>, String>
{
    runinator_adapter_client::kinds().await
}

#[allow(
    clippy::result_large_err,
    reason = "handler helpers pass the shared Axum reply through without allocating an error wrapper"
)]
fn org_id(ctx: &AuthContext) -> Result<Uuid, (StatusCode, Json<ApiResponse>)> {
    ctx.org_id
        .ok_or_else(|| bad_request("an organization must be selected"))
}

fn forbidden() -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::ApiError(runinator_ws_core::models::ApiError {
            message: "forbidden".into(),
            path: None,
            expected: None,
            actual: None,
        })),
    )
}

#[allow(
    clippy::result_large_err,
    reason = "handler helpers pass the shared Axum reply through without allocating an error wrapper"
)]
fn require_scope(
    ctx: &AuthContext,
    action: Action,
) -> Result<Uuid, (StatusCode, Json<ApiResponse>)> {
    let org_id = org_id(ctx)?;
    ctx.require_scope_action(action, ctx.selected_scope())?;
    Ok(org_id)
}

/// Platform administrators may inspect adapters without selecting an organization. In that
/// platform-wide view `None` deliberately means every organization; ordinary callers remain
/// restricted to their selected organization.
#[allow(
    clippy::result_large_err,
    reason = "handler helpers pass the shared Axum reply through without allocating an error wrapper"
)]
fn adapter_list_scope(
    ctx: &AuthContext,
    action: Action,
) -> Result<Option<Uuid>, (StatusCode, Json<ApiResponse>)> {
    if ctx.is_platform_admin() {
        ctx.require_scope_action(action, ctx.selected_scope())?;
        Ok(ctx.org_id)
    } else {
        Ok(Some(require_scope(ctx, action)?))
    }
}

async fn authorized_adapter<T: OrchestrationStore + AuthorizationStore>(
    db: &T,
    operations: &AdapterOperations<T>,
    ctx: &AuthContext,
    adapter_id: Uuid,
    action: Action,
) -> Result<AdapterDefinition, (StatusCode, Json<ApiResponse>)> {
    let permission = match action {
        Action::View => Permission::View,
        Action::Own => Permission::Own,
        _ => Permission::Edit,
    };
    AuthzChecker::new(db, ctx)
        .require_resource(ResourceType::OrchestrationAdapter, adapter_id, permission)
        .await?;
    match operations.fetch(adapter_id).await {
        Ok(Some(adapter)) => Ok(adapter),
        Ok(_) => Err(not_found("adapter not found")),
        Err(error) => Err(api_error(error.to_string())),
    }
}

async fn validate_adapter_secret_access<T: AuthorizationStore>(
    db: &T,
    ctx: Option<&AuthContext>,
    adapter_id: Option<Uuid>,
    org_id: Uuid,
    bindings: &BTreeMap<String, Uuid>,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    let tenant = ScopeRef::new(ScopeKind::Organization, Some(org_id)).unwrap();
    let prospective_owner = ctx
        .and_then(|ctx| match (ctx.kind, ctx.principal_id) {
            (PrincipalKind::User, Some(id)) => ScopeRef::new(ScopeKind::User, Some(id)),
            _ => None,
        })
        .unwrap_or(tenant);
    for setting_id in bindings.values().copied() {
        if let Some(ctx) = ctx {
            AuthzChecker::new(db, ctx)
                .require_resource(ResourceType::Setting, setting_id, Permission::Run)
                .await?;
        }
        let allowed = match adapter_id {
            Some(adapter_id) => {
                runinator_store::resource_access::resource_can_consume(
                    db,
                    ResourceType::OrchestrationAdapter,
                    adapter_id,
                    ResourceType::Setting,
                    setting_id,
                )
                .await
            }
            None => {
                runinator_store::resource_access::owner_can_consume(
                    db,
                    prospective_owner,
                    tenant,
                    ResourceType::Setting,
                    setting_id,
                )
                .await
            }
        }
        .map_err(|error| api_error(error.to_string()))?;
        if !allowed {
            return Err(bad_request(format!(
                "adapter is not permitted to use setting {setting_id}"
            )));
        }
    }
    Ok(())
}

async fn current_revision<T: OrchestrationStore>(
    operations: &AdapterOperations<T>,
    adapter: &AdapterDefinition,
) -> Result<runinator_models::orchestration::AdapterRevision, (StatusCode, Json<ApiResponse>)> {
    match operations.current_revision(adapter).await {
        Ok(Some(revision)) => Ok(revision),
        Ok(None) => Err(api_error("current adapter revision is missing")),
        Err(error) => Err(api_error(error.to_string())),
    }
}

fn valid_github_repositories(configuration: &runinator_models::value::Value) -> bool {
    configuration
        .get("repositories")
        .and_then(|value| value.as_array())
        .is_some_and(|items| {
            !items.is_empty()
                && items.iter().all(|item| {
                    item.as_str().is_some_and(|value| {
                        let value = value.trim();
                        !value.is_empty()
                            && value.split_once('/').is_some_and(|(owner, repository)| {
                                !owner.is_empty() && !repository.is_empty()
                            })
                    })
                })
        })
}

fn validate_definition(
    request: &AdapterApplyRequest,
    kind: &AdapterKindMetadata,
) -> Result<(), String> {
    if request.name.trim().is_empty() {
        return Err("adapter name is required".into());
    }
    if request.kind_version != kind.version {
        return Err(format!(
            "adapter kind '{}' is loaded at version {}, not {}",
            kind.kind, kind.version, request.kind_version
        ));
    }
    if request.transport == AdapterTransport::Polling {
        // an absent interval takes the default; a present one must be a valid integer in range.
        // folding a negative or non-numeric value into the default would accept a configuration the
        // poll loop then silently clamps to something the author never asked for.
        match request.configuration.get("poll_interval_seconds") {
            None | Some(runinator_models::value::Value::Null) => {}
            Some(value) => {
                let interval = value.as_i64().ok_or(
                    "poll_interval_seconds must be an integer number of seconds".to_string(),
                )?;
                if !(30..=3_600).contains(&interval) {
                    return Err("poll_interval_seconds must be between 30 and 3600".into());
                }
            }
        }
        match request.kind.as_str() {
            "github" if valid_github_repositories(&request.configuration) && request.secret_bindings.contains_key("access_token") => return Ok(()),
            "jira" if ["instance_id", "base_url", "email", "jql"].iter().all(|field| request.configuration.get(*field).and_then(|value| value.as_str()).is_some_and(|value| !value.trim().is_empty())) && request.secret_bindings.contains_key("api_token") => return Ok(()),
            "github" => return Err("GitHub polling requires repositories and access_token secret binding".into()),
            "jira" => return Err("Jira polling requires instance_id, base_url, email, jql, and api_token secret binding".into()),
            _ => return Err("only GitHub and Jira adapters support polling".into()),
        }
    }
    for field in &kind.fields {
        if field.secret {
            if field.required && !request.secret_bindings.contains_key(&field.name) {
                return Err(format!("secret binding '{}' is required", field.name));
            }
            continue;
        }
        let Some(value) = request.configuration.get(&field.name) else {
            if !field.required {
                continue;
            }
            return Err(format!("configuration field '{}' is required", field.name));
        };
        if value.is_null() && !field.required {
            continue;
        }
        field.value_type.validate_value(value).map_err(|error| {
            format!(
                "configuration field '{}' does not match its declared type: {error}",
                field.name
            )
        })?;
        if field.required && value.as_str().is_some_and(|value| value.trim().is_empty()) {
            return Err(format!(
                "configuration field '{}' cannot be empty",
                field.name
            ));
        }
    }
    Ok(())
}

fn identity_projection(
    kind: &str,
    configuration: &runinator_models::value::Value,
) -> serde_json::Value {
    let configuration = serde_json::to_value(configuration).unwrap_or_default();
    let fields: &[&str] = match kind {
        "generic_webhook" => &[
            "delivery_id_pointer",
            "scope_pointer",
            "correlation_pointer",
        ],
        "jira" => &["instance_id"],
        "github" => &["repositories"],
        _ => &[],
    };
    serde_json::Value::Object(
        fields
            .iter()
            .filter_map(|field| {
                configuration
                    .get(*field)
                    .cloned()
                    .map(|value| ((*field).to_owned(), value))
            })
            .collect(),
    )
}

fn webhook_body_limit() -> usize {
    std::env::var("RUNINATOR_ADAPTER_WEBHOOK_BODY_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1024 * 1024)
}

fn allowed_webhook_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    let allowed = webhook_header_allowlist();
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str().to_ascii_lowercase();
            (allowed.contains(&name))
                .then(|| value.to_str().ok().map(|value| (name, value.to_owned())))
                .flatten()
        })
        .collect()
}

fn webhook_header_allowlist() -> std::collections::BTreeSet<String> {
    let configured = std::env::var("RUNINATOR_ADAPTER_WEBHOOK_HEADER_ALLOWLIST")
        .unwrap_or_else(|_| {
            "authorization,content-type,x-runinator-signature,x-hub-signature-256,x-github-delivery,x-github-event,x-atlassian-webhook-identifier".into()
        });
    configured
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .collect()
}

pub async fn kinds<T: RbacStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = adapter_list_scope(&ctx, Action::View) {
        return reply;
    }
    match catalog().await {
        Ok(entries) => (StatusCode::OK, Json(ApiResponse::AdapterKindList(entries))),
        Err(error) => api_error(error),
    }
}

pub async fn list<T: OrchestrationStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    let org_id = match adapter_list_scope(&ctx, Action::View) {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let operations = AdapterOperations::new(db.clone());
    match operations.list(org_id).await {
        Ok(mut values) => {
            if let Some(visible) = match AuthzChecker::new(db.as_ref(), &ctx)
                .visible_resource_ids(ResourceType::OrchestrationAdapter)
                .await
            {
                Ok(value) => value,
                Err(reply) => return reply,
            } {
                values.retain(|value| visible.contains(&value.id));
            }
            (
                StatusCode::OK,
                Json(ApiResponse::OrchestrationAdapterList(values)),
            )
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn get_one<T: OrchestrationStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let operations = AdapterOperations::new(db.clone());
    match authorized_adapter(db.as_ref(), &operations, &ctx, id, Action::View).await {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationAdapter(value)),
        ),
        Err(reply) => reply,
    }
}

pub async fn revisions<T: OrchestrationStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let operations = AdapterOperations::new(db.clone());
    if let Err(reply) = authorized_adapter(db.as_ref(), &operations, &ctx, id, Action::View).await {
        return reply;
    }
    match operations.revisions(id).await {
        Ok(values) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationAdapterRevisionList(values)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn poll_status<T: OrchestrationStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let operations = AdapterOperations::new(db.clone());
    if let Err(reply) = authorized_adapter(db.as_ref(), &operations, &ctx, id, Action::View).await {
        return reply;
    }
    match operations.poll_status(id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(
                serde_json::to_value(status).unwrap_or_default().into(),
            )),
        ),
        Ok(None) => not_found("adapter is not configured for polling"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn create<T: OrchestrationStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(publisher): Extension<UiEventPublisher>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<AdapterApplyRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let org_id = match require_scope(&ctx, Action::Edit) {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let kinds = match catalog().await {
        Ok(values) => values,
        Err(error) => return api_error(error),
    };
    let Some(kind) = kinds
        .into_iter()
        .find(|entry| entry.healthy && entry.metadata.kind == request.kind)
        .map(|entry| entry.metadata)
    else {
        return bad_request(format!("adapter kind '{}' is not loaded", request.kind));
    };
    if let Err(error) = validate_definition(&request, &kind) {
        return bad_request(error);
    }
    if let Err(reply) = validate_adapter_secret_access(
        db.as_ref(),
        Some(&ctx),
        None,
        org_id,
        &request.secret_bindings,
    )
    .await
    {
        return reply;
    }
    let now = Utc::now();
    let adapter_id = Uuid::now_v7();
    let operations = AdapterOperations::new(db.clone());
    match operations
        .create(
            NewAdapterDefinition {
                id: adapter_id,
                org_id,
                name: request.name,
                kind: request.kind,
                kind_version: request.kind_version,
                transport: request.transport,
                endpoint_identity: Uuid::new_v4().to_string(),
                configuration: request.configuration,
                secret_bindings: request.secret_bindings,
                identity_configuration: request.identity_configuration,
                actor_id: ctx.principal_id,
            },
            now,
        )
        .await
    {
        Ok((adapter, _)) => {
            if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                .grant_resource_owner(ResourceType::OrchestrationAdapter, adapter.id)
                .await
            {
                return reply;
            }
            emit_adapter(&publisher, adapter.id, Some(org_id));
            (
                StatusCode::CREATED,
                Json(ApiResponse::OrchestrationAdapter(adapter)),
            )
        }
        Err(error) => bad_request(error.to_string()),
    }
}

pub async fn update<T: OrchestrationStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(publisher): Extension<UiEventPublisher>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<AdapterApplyRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let operations = AdapterOperations::new(db.clone());
    let adapter = match authorized_adapter(db.as_ref(), &operations, &ctx, id, Action::Edit).await {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    if request.kind != adapter.kind {
        return bad_request("adapter kind cannot be changed; clone the adapter instead");
    }
    let kinds = match catalog().await {
        Ok(values) => values,
        Err(error) => return api_error(error),
    };
    let Some(kind) = kinds
        .into_iter()
        .find(|entry| entry.healthy && entry.metadata.kind == request.kind)
        .map(|entry| entry.metadata)
    else {
        return bad_request(format!("adapter kind '{}' is not loaded", request.kind));
    };
    if let Err(error) = validate_definition(&request, &kind) {
        return bad_request(error);
    }
    if let Err(reply) = validate_adapter_secret_access(
        db.as_ref(),
        Some(&ctx),
        Some(id),
        adapter.org_id,
        &request.secret_bindings,
    )
    .await
    {
        return reply;
    }
    if adapter.has_admitted_binding {
        let current = match current_revision(&operations, &adapter).await {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        if current.transport != request.transport {
            return bad_request(
                "adapter transport is immutable after its first admitted binding; clone the adapter instead",
            );
        }
        if identity_projection(&adapter.kind, &current.configuration)
            != identity_projection(&adapter.kind, &request.configuration)
        {
            return bad_request(
                "adapter identity extraction is immutable after its first admitted binding; clone the adapter instead",
            );
        }
    }
    let expected_revision = request
        .expected_revision
        .unwrap_or(adapter.current_revision);
    match operations
        .create_revision(
            NewAdapterRevision {
                id: Uuid::now_v7(),
                adapter_id: id,
                expected_revision,
                kind_version: request.kind_version,
                transport: request.transport,
                configuration: request.configuration,
                secret_bindings: request.secret_bindings,
                identity_configuration: request.identity_configuration,
                actor_id: ctx.principal_id,
            },
            Utc::now(),
        )
        .await
    {
        Ok(Some((adapter, _))) => {
            emit_adapter(&publisher, adapter.id, Some(adapter.org_id));
            (
                StatusCode::OK,
                Json(ApiResponse::OrchestrationAdapter(adapter)),
            )
        }
        Ok(None) => (
            StatusCode::CONFLICT,
            Json(ApiResponse::ApiError(runinator_ws_core::models::ApiError {
                message: "adapter revision changed; refresh and retry".into(),
                path: None,
                expected: None,
                actual: None,
            })),
        ),
        Err(error) => bad_request(error.to_string()),
    }
}

pub async fn set_enabled<T: OrchestrationStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(publisher): Extension<UiEventPublisher>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<AdapterEnableRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let operations = AdapterOperations::new(db.clone());
    let authorized =
        match authorized_adapter(db.as_ref(), &operations, &ctx, id, Action::Edit).await {
            Ok(adapter) => adapter,
            Err(reply) => return reply,
        };
    match operations
        .set_enabled(id, request.enabled, Utc::now())
        .await
    {
        Ok(Some(adapter)) => {
            emit_adapter(&publisher, adapter.id, Some(authorized.org_id));
            (
                StatusCode::OK,
                Json(ApiResponse::OrchestrationAdapter(adapter)),
            )
        }
        Ok(None) => not_found("adapter not found"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn remove<T: OrchestrationStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(publisher): Extension<UiEventPublisher>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let operations = AdapterOperations::new(db.clone());
    let adapter = match authorized_adapter(db.as_ref(), &operations, &ctx, id, Action::Own).await {
        Ok(adapter) => adapter,
        Err(reply) => return reply,
    };
    match operations.delete(id).await {
        Ok(true) => {
            emit_adapter(&publisher, id, Some(adapter.org_id));
            (
                StatusCode::OK,
                Json(ApiResponse::TaskResponse(TaskResponse {
                    success: true,
                    message: "adapter deleted".into(),
                })),
            )
        }
        Ok(false) => {
            bad_request("an adapter that admitted bindings cannot be deleted; disable or clone it")
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn test<
    T: OrchestrationStore
        + AuthorizationStore
        + SettingStore
        + RuntimeStore
        + DefinitionStore
        + IngressStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<AdapterTestRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let operations = AdapterOperations::new(db.clone());
    let adapter = match authorized_adapter(db.as_ref(), &operations, &ctx, id, Action::Edit).await {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let revision = match current_revision(&operations, &adapter).await {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let bindings = request.secret_bindings.unwrap_or(revision.secret_bindings);
    if let Err(reply) =
        validate_adapter_secret_access(db.as_ref(), Some(&ctx), Some(id), adapter.org_id, &bindings)
            .await
    {
        return reply;
    }
    let secrets = match operations.resolve_secrets(adapter.org_id, &bindings).await {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
    if revision.transport == AdapterTransport::Polling {
        let configuration = request.configuration.unwrap_or(revision.configuration);
        let response: AdapterPollResponse = match runinator_adapter_client::poll(
            &adapter.kind,
            AdapterPollRequest {
                configuration: serde_json::to_value(configuration).unwrap_or_default(),
                secrets,
                checkpoint: serde_json::Value::Null,
                initialize: false,
            },
        )
        .await
        {
            Ok(value) => value,
            Err(error) => return api_error(error),
        };
        let mut previews = Vec::new();
        for event in &response.events {
            match operations.preview_event(&adapter, event).await {
                Ok(preview) => previews.push(preview),
                Err(error) => return api_error(error),
            }
        }
        return (StatusCode::OK, Json(ApiResponse::JsonValue(serde_json::json!({
            "verified": response.error.is_none(), "events": response.events,
            "errors": response.error.into_iter().collect::<Vec<_>>(), "previews": previews,
            "checkpoint": response.checkpoint, "retry_after_seconds": response.retry_after_seconds,
        }).into())));
    }
    let headers = request
        .headers
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect();
    let adapter_request = AdapterRequest {
        method: "POST".into(),
        headers,
        body_base64: request.body_base64,
        configuration: serde_json::to_value(
            request.configuration.unwrap_or(revision.configuration),
        )
        .unwrap_or_default(),
        secrets,
    };
    match runinator_adapter_client::verify_normalize(&adapter.kind, adapter_request).await {
        Ok(normalized) => {
            let mut previews = Vec::new();
            if normalized.verified {
                for event in &normalized.events {
                    match operations.preview_event(&adapter, event).await {
                        Ok(preview) => previews.push(preview),
                        Err(error) => return api_error(error),
                    }
                }
            }
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(
                    serde_json::json!({
                        "verified": normalized.verified,
                        "events": normalized.events,
                        "errors": normalized.errors,
                        "previews": previews,
                    })
                    .into(),
                )),
            )
        }
        Err(error) => api_error(error),
    }
}

pub async fn health<T: RbacStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if !ctx.is_platform_admin() {
        return forbidden();
    }
    match runinator_adapter_client::health().await {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(
                serde_json::json!({
                    "host": value,
                    "web_service": {
                        "adapter_host_url": runinator_adapter_client::host_url(),
                        "adapter_host_token_configured": runinator_adapter_client::host_token().is_ok(),
                        "webhook_body_limit_bytes": webhook_body_limit(),
                        "webhook_header_allowlist": webhook_header_allowlist(),
                    }
                })
                .into(),
            )),
        ),
        Err(error) => api_error(error),
    }
}

pub async fn reload<T: RbacStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if !ctx.is_platform_admin() {
        return forbidden();
    }
    match runinator_adapter_client::reload().await {
        Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value.into()))),
        Err(error) => api_error(error),
    }
}

/// Public data-plane endpoint. The selected adapter must authenticate and normalize the request
/// before any event is allowed to reach durable ingress.
pub async fn webhook<
    T: OrchestrationStore
        + AuthorizationStore
        + SettingStore
        + RuntimeStore
        + DefinitionStore
        + IngressStore
        + runinator_store::roles::RbacStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(pipelines): Extension<Arc<PipelineOperations<T>>>,
    Path(endpoint): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if body.len() > webhook_body_limit() {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ApiResponse::ApiError(runinator_ws_core::models::ApiError {
                message: "webhook body exceeds the configured adapter limit".into(),
                path: None,
                expected: None,
                actual: Some(body.len().to_string()),
            })),
        );
    }
    let operations = AdapterOperations::new(db.clone());
    let adapter = match endpoint.parse::<Uuid>() {
        Ok(id) => operations.fetch(id).await,
        Err(_) => operations.fetch_by_endpoint(endpoint).await,
    };
    let adapter = match adapter {
        Ok(Some(adapter)) if adapter.enabled => adapter,
        Ok(Some(_)) => return not_found("adapter is disabled"),
        Ok(None) => return not_found("adapter not found"),
        Err(error) => return api_error(error.to_string()),
    };
    let revision = match current_revision(&operations, &adapter).await {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    if revision.transport != AdapterTransport::Webhook {
        return not_found("adapter is configured for polling, not webhook delivery");
    }
    if let Err(reply) = validate_adapter_secret_access(
        db.as_ref(),
        None,
        Some(adapter.id),
        adapter.org_id,
        &revision.secret_bindings,
    )
    .await
    {
        return reply;
    }
    let secrets = match operations
        .resolve_secrets(adapter.org_id, &revision.secret_bindings)
        .await
    {
        Ok(value) => value,
        Err(error) => return api_error(error),
    };
    use base64::Engine;
    let request = AdapterRequest {
        method: "POST".into(),
        headers: allowed_webhook_headers(&headers),
        body_base64: base64::engine::general_purpose::STANDARD.encode(body),
        configuration: serde_json::to_value(revision.configuration.clone()).unwrap_or_default(),
        secrets,
    };
    let normalized = match runinator_adapter_client::verify_normalize(&adapter.kind, request).await
    {
        Ok(value) => value,
        Err(error) => return api_error(error),
    };
    if !normalized.verified {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ApiResponse::ApiError(runinator_ws_core::models::ApiError {
                message: normalized.errors.join("; "),
                path: None,
                expected: None,
                actual: None,
            })),
        );
    }
    let mut outcomes = Vec::new();
    for mut event in normalized.events {
        if let Err(error) = event.validate_identity() {
            return bad_request(error);
        }
        if let Err(error) = operations
            .resolve_correlation_alias(adapter.org_id, &mut event)
            .await
        {
            return bad_request(error);
        }
        if let Some(payload) = event.payload.as_object_mut() {
            if let Some(subject_revision) = event.subject_revision.clone() {
                payload.insert("subject_revision".into(), subject_revision.into());
            }
            if !event.provenance.is_null() {
                payload.insert("provenance".into(), event.provenance.clone());
            }
        }
        let pipeline_id = match operations.pipeline_for_event(&adapter, &event).await {
            Ok(value) => value,
            Err(error) => return bad_request(error),
        };
        match runinator_store::resource_access::resource_can_consume(
            db.as_ref(),
            ResourceType::Pipeline,
            pipeline_id,
            ResourceType::OrchestrationAdapter,
            adapter.id,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => return bad_request("pipeline is not permitted to use this adapter"),
            Err(error) => return api_error(error.to_string()),
        }
        let reply = process_pipeline_ingress(
            pipelines.clone(),
            pipeline_id,
            Some(adapter.org_id),
            IngressEventRequest {
                source: format!("adapter:{}:{}", adapter.id, event.source),
                event_id: event.delivery_id,
                event_type: event.event_type,
                correlation_key: event.correlation_key,
                payload: event.payload,
                provenance: event.provenance,
                occurred_at: event.occurred_at,
            },
            Some((adapter.id, revision.revision)),
        )
        .await;
        if !reply.0.is_success() {
            return reply;
        }
        outcomes.push(serde_json::to_value(&reply.1.0).unwrap_or_default());
    }
    (
        StatusCode::ACCEPTED,
        Json(ApiResponse::JsonValue(
            serde_json::json!({
                "adapter_id": adapter.id,
                "adapter_revision": revision.revision,
                "outcomes": outcomes,
            })
            .into(),
        )),
    )
}

pub fn routes<T>(pool: Arc<T>, publisher: UiEventPublisher) -> axum::Router
where
    T: OrchestrationStore
        + AuthorizationStore
        + SettingStore
        + RuntimeStore
        + DefinitionStore
        + IngressStore
        + ScheduleStore
        + WorkflowVmStore,
{
    axum::Router::new()
        .route("/orchestrations/adapters/kinds", get(kinds::<T>))
        .route("/orchestrations/adapters/health", get(health::<T>))
        .route("/orchestrations/adapters/reload", post(reload::<T>))
        .route("/orchestrations/adapters", get(list::<T>).post(create::<T>))
        .route(
            "/orchestrations/adapters/{id}",
            get(get_one::<T>).post(update::<T>).delete(remove::<T>),
        )
        .route(
            "/orchestrations/adapters/{id}/revisions",
            get(revisions::<T>),
        )
        .route(
            "/orchestrations/adapters/{id}/poll-status",
            get(poll_status::<T>),
        )
        .route(
            "/orchestrations/adapters/{id}/enabled",
            post(set_enabled::<T>),
        )
        .route("/orchestrations/adapters/{id}/test", post(test::<T>))
        .route("/webhooks/orchestration/{adapter_id}", post(webhook::<T>))
        .layer(Extension(pool))
        .layer(Extension(publisher))
}

/// OpenAPI entries for adapter catalog, configuration, diagnostics, and verified webhook ingress.
pub const DOCS: &[EndpointDoc] = &[
    endpoint_with_policy(
        "get",
        "/orchestrations/adapters/kinds",
        "Orchestration Adapters",
        "List adapter kinds",
        "Lists loaded and failed adapter kinds with typed configuration metadata, origin, capabilities, and health.",
        EndpointPolicy::ScopedAction(Action::View),
        None,
        &[],
        200,
        "adapter kind catalog",
        Example::AdapterKindList,
    ),
    endpoint_with_policy(
        "get",
        "/orchestrations/adapters/health",
        "Orchestration Adapters",
        "Inspect adapter host health",
        "Returns loopback adapter-host health and load diagnostics. Platform administrator access is required.",
        EndpointPolicy::Authenticated,
        None,
        &[],
        200,
        "adapter host health",
        Example::AdapterHealth,
    ),
    endpoint_with_policy(
        "post",
        "/orchestrations/adapters/reload",
        "Orchestration Adapters",
        "Reload adapter plugins",
        "Reloads configured filesystem plugin paths in the out-of-process adapter host. Platform administrator access is required.",
        EndpointPolicy::Authenticated,
        None,
        &[],
        200,
        "adapter host reload result",
        Example::AdapterHealth,
    ),
    endpoint_with_policy(
        "get",
        "/orchestrations/adapters",
        "Orchestration Adapters",
        "List adapter definitions",
        "Lists adapter instances in the selected organization, or every organization for a platform administrator with no organization selected.",
        EndpointPolicy::ScopedAction(Action::View),
        None,
        &[],
        200,
        "adapter definitions",
        Example::AdapterDefinitionList,
    ),
    endpoint_with_policy(
        "post",
        "/orchestrations/adapters",
        "Orchestration Adapters",
        "Create an adapter definition",
        "Creates an org-scoped adapter and immutable revision after validating it against loaded kind metadata.",
        EndpointPolicy::ScopedAction(Action::Edit),
        json_body(
            "Typed configuration and stored Secret bindings for the adapter.",
            Example::AdapterApply,
        ),
        &[],
        201,
        "adapter definition created",
        Example::AdapterDefinition,
    ),
    endpoint_with_policy(
        "get",
        "/orchestrations/adapters/{id}",
        "Orchestration Adapters",
        "Show an adapter definition",
        "Returns one adapter instance without exposing secret values.",
        EndpointPolicy::ScopedAction(Action::View),
        None,
        &[],
        200,
        "adapter definition",
        Example::AdapterDefinition,
    ),
    endpoint_with_policy(
        "post",
        "/orchestrations/adapters/{id}",
        "Orchestration Adapters",
        "Create an adapter revision",
        "Validates and appends an immutable adapter revision using optimistic revision matching. Identity extraction cannot change after first admission.",
        EndpointPolicy::ScopedAction(Action::Edit),
        json_body(
            "Complete adapter configuration with expected revision.",
            Example::AdapterApply,
        ),
        &[],
        200,
        "updated adapter definition",
        Example::AdapterDefinition,
    ),
    endpoint_with_policy(
        "delete",
        "/orchestrations/adapters/{id}",
        "Orchestration Adapters",
        "Delete an unused adapter",
        "Deletes an adapter only if it has never admitted a binding; otherwise disable it or clone it.",
        EndpointPolicy::ScopedAction(Action::Own),
        None,
        &[],
        200,
        "adapter deleted",
        Example::TaskResponse,
    ),
    endpoint_with_policy(
        "get",
        "/orchestrations/adapters/{id}/revisions",
        "Orchestration Adapters",
        "List adapter revisions",
        "Lists the immutable configuration history for one adapter.",
        EndpointPolicy::ScopedAction(Action::View),
        None,
        &[],
        200,
        "adapter revisions",
        Example::AdapterRevisionList,
    ),
    endpoint_with_policy(
        "get",
        "/orchestrations/adapters/{id}/poll-status",
        "Orchestration Adapters",
        "Show polling adapter status",
        "Returns the durable checkpoint, next scheduled poll, claim lease, and last attempt, success, or error for a polling adapter.",
        EndpointPolicy::ScopedAction(Action::View),
        None,
        &[],
        200,
        "polling adapter status",
        Example::AdapterPollStatus,
    ),
    endpoint_with_policy(
        "post",
        "/orchestrations/adapters/{id}/enabled",
        "Orchestration Adapters",
        "Enable or disable an adapter",
        "Changes whether the adapter accepts new webhook requests or polling claims without altering its immutable revisions.",
        EndpointPolicy::ScopedAction(Action::Edit),
        json_body("Desired enabled state.", Example::AdapterEnable),
        &[],
        200,
        "updated adapter definition",
        Example::AdapterDefinition,
    ),
    endpoint_with_policy(
        "post",
        "/orchestrations/adapters/{id}/test",
        "Orchestration Adapters",
        "Test adapter verification and routing",
        "Verifies a webhook sample or performs a non-persisting polling preview, then normalizes events and previews matching routes and candidate intents.",
        EndpointPolicy::ScopedAction(Action::Edit),
        json_body(
            "Sample headers and base64 body, with optional temporary configuration and Secret bindings.",
            Example::AdapterTest,
        ),
        &[],
        200,
        "verification, normalized events, and route preview",
        Example::AdapterTestResult,
    ),
    endpoint_with_policy(
        "post",
        "/webhooks/orchestration/{adapter_id}",
        "Orchestration Adapters",
        "Receive a verified adapter webhook",
        "Public provider endpoint. The selected adapter must authenticate and normalize the bounded request before any event reaches durable ingress.",
        EndpointPolicy::Public,
        None,
        &[],
        202,
        "adapter delivery outcomes",
        Example::AdapterWebhookResponse,
    ),
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{identity_projection, validate_definition};
    use runinator_models::{
        json,
        orchestration::{AdapterConfigurationField, AdapterKindMetadata, AdapterTransport},
        types::RuninatorType,
        value::Value,
    };
    use runinator_ws_core::models::AdapterApplyRequest;
    use uuid::Uuid;

    #[test]
    fn generic_identity_projection_ignores_non_identity_configuration() {
        let first = json!({
            "delivery_id_pointer": "/delivery",
            "scope_pointer": "/tenant",
            "correlation_pointer": "/subject/id",
            "event_pointer": "/event"
        });
        let second = json!({
            "delivery_id_pointer": "/delivery",
            "scope_pointer": "/tenant",
            "correlation_pointer": "/subject/id",
            "event_pointer": "/kind",
            "payload_pointer": "/payload"
        });
        assert_eq!(
            identity_projection("generic_webhook", &first),
            identity_projection("generic_webhook", &second)
        );
    }

    #[test]
    fn identity_projection_tracks_generic_pointers_and_jira_instance() {
        assert_ne!(
            identity_projection(
                "generic_webhook",
                &json!({
                    "delivery_id_pointer": "/delivery",
                    "scope_pointer": "/tenant",
                    "correlation_pointer": "/subject/id"
                })
            ),
            identity_projection(
                "generic_webhook",
                &json!({
                    "delivery_id_pointer": "/delivery",
                    "scope_pointer": "/tenant",
                    "correlation_pointer": "/new/id"
                })
            )
        );
        assert_ne!(
            identity_projection("jira", &json!({ "instance_id": "first" })),
            identity_projection("jira", &json!({ "instance_id": "second" }))
        );
    }

    #[test]
    fn adapter_configuration_is_validated_against_kind_metadata() {
        let metadata = AdapterKindMetadata {
            kind: "example".into(),
            version: "1".into(),
            display_name: "Example".into(),
            description: None,
            fields: vec![AdapterConfigurationField {
                name: "pointer".into(),
                value_type: RuninatorType::String,
                required: true,
                secret: false,
                description: None,
                default: Value::Null,
            }],
            event_names: vec![],
            canonical_pointers: vec![],
            capabilities: vec![],
            setup_instructions: vec![],
        };
        let request = AdapterApplyRequest {
            name: "adapter".into(),
            kind: "example".into(),
            kind_version: "1".into(),
            transport: AdapterTransport::Webhook,
            configuration: json!({ "pointer": 42 }),
            secret_bindings: BTreeMap::new(),
            identity_configuration: Value::Null,
            expected_revision: None,
        };
        assert!(
            validate_definition(&request, &metadata)
                .unwrap_err()
                .contains("declared type")
        );
    }

    #[test]
    fn polling_uses_transport_specific_configuration() {
        let metadata = AdapterKindMetadata {
            kind: "github".into(),
            version: "1".into(),
            display_name: "GitHub".into(),
            description: None,
            fields: vec![],
            event_names: vec![],
            canonical_pointers: vec![],
            capabilities: vec![],
            setup_instructions: vec![],
        };
        let github = AdapterApplyRequest {
            name: "GitHub poller".into(),
            kind: "github".into(),
            kind_version: "1".into(),
            transport: AdapterTransport::Polling,
            configuration: json!({
                "repositories": ["octo/example"],
                "poll_interval_seconds": 60
            }),
            secret_bindings: BTreeMap::from([("access_token".into(), Uuid::new_v4())]),
            identity_configuration: Value::Null,
            expected_revision: None,
        };
        assert!(validate_definition(&github, &metadata).is_ok());

        let mut missing_token = github;
        missing_token.secret_bindings.clear();
        assert!(
            validate_definition(&missing_token, &metadata)
                .unwrap_err()
                .contains("access_token")
        );

        let unsupported = AdapterApplyRequest {
            name: "Unsupported poller".into(),
            kind: "generic_webhook".into(),
            kind_version: "1".into(),
            transport: AdapterTransport::Polling,
            configuration: json!({ "poll_interval_seconds": 60 }),
            secret_bindings: BTreeMap::new(),
            identity_configuration: Value::Null,
            expected_revision: None,
        };
        assert!(
            validate_definition(&unsupported, &metadata)
                .unwrap_err()
                .contains("only GitHub and Jira")
        );
    }
}
