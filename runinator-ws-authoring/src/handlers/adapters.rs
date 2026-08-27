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
use runinator_adapter_contract::{AdapterRequest, AdapterResponse};
use runinator_engine::services::PipelineOperations;
use runinator_models::{
    auth::AuthContext,
    orchestration::{
        AdapterDefinition, AdapterKindMetadata, AdapterRevision, IngressAction, IngressLifecycle,
        IngressPolicy, IngressTargetKind,
    },
    rbac::Action,
    settings::SettingKind,
    web::TaskResponse,
};
use runinator_secrets::{secret_cipher::SecretCipher, stored_secret::StoredSecret};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, IngressStore, NewAdapterDefinition, NewAdapterRevision,
        OrchestrationStore, RbacStore, ScheduleStore, SettingStore, WorkflowVmStore,
    },
};
use runinator_ws_core::{
    models::{
        AdapterApplyRequest, AdapterEnableRequest, AdapterTestRequest, ApiResponse,
        IngressEventRequest,
    },
    responses::{api_error, bad_request, not_found},
};
use runinator_ws_middleware::authz::AuthContextExt;
use serde::Deserialize;
use uuid::Uuid;

use super::pipelines::process_pipeline_ingress;

#[derive(Debug, Clone, Deserialize)]
struct HostCatalogEntry {
    metadata: AdapterKindMetadata,
    #[allow(dead_code)]
    origin: String,
    healthy: bool,
    error: Option<String>,
}

fn host_url() -> String {
    std::env::var("RUNINATOR_ADAPTER_HOST_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8790".into())
        .trim_end_matches('/')
        .to_owned()
}

fn host_token() -> Result<String, String> {
    std::env::var("RUNINATOR_ADAPTER_HOST_TOKEN")
        .map_err(|_| "RUNINATOR_ADAPTER_HOST_TOKEN is not configured".into())
}

async fn host_get(path: &str) -> Result<serde_json::Value, String> {
    let token = host_token()?;
    let response = reqwest::Client::new()
        .get(format!("{}{path}", host_url()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("adapter host is unavailable: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("adapter host returned {}", response.status()));
    }
    response.json().await.map_err(|error| error.to_string())
}

async fn host_post(path: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let token = host_token()?;
    let response = reqwest::Client::new()
        .post(format!("{}{path}", host_url()))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("adapter host is unavailable: {error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let message = response.text().await.unwrap_or_default();
        return Err(format!("adapter host returned {status}: {message}"));
    }
    response.json().await.map_err(|error| error.to_string())
}

async fn catalog() -> Result<Vec<HostCatalogEntry>, String> {
    serde_json::from_value(host_get("/kinds").await?).map_err(|error| error.to_string())
}

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

fn require_scope(
    ctx: &AuthContext,
    action: Action,
) -> Result<Uuid, (StatusCode, Json<ApiResponse>)> {
    let org_id = org_id(ctx)?;
    ctx.require_scope_action(action, ctx.selected_scope())?;
    Ok(org_id)
}

async fn authorized_adapter<T: OrchestrationStore>(
    db: &T,
    ctx: &AuthContext,
    adapter_id: Uuid,
    action: Action,
) -> Result<AdapterDefinition, (StatusCode, Json<ApiResponse>)> {
    let org_id = require_scope(ctx, action)?;
    match db.fetch_orchestration_adapter(adapter_id).await {
        Ok(Some(adapter)) if adapter.org_id == org_id => Ok(adapter),
        Ok(_) => Err(not_found("adapter not found")),
        Err(error) => Err(api_error(error.to_string())),
    }
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
    for field in &kind.fields {
        if !field.required {
            continue;
        }
        if field.secret {
            if !request.secret_bindings.contains_key(&field.name) {
                return Err(format!("secret binding '{}' is required", field.name));
            }
        } else if request.configuration.get(&field.name).is_none() {
            return Err(format!("configuration field '{}' is required", field.name));
        }
    }
    Ok(())
}

async fn current_revision<T: OrchestrationStore>(
    db: &T,
    adapter: &AdapterDefinition,
) -> Result<AdapterRevision, (StatusCode, Json<ApiResponse>)> {
    match db
        .fetch_orchestration_adapter_revision(adapter.id, adapter.current_revision)
        .await
    {
        Ok(Some(revision)) => Ok(revision),
        Ok(None) => Err(api_error("current adapter revision is missing")),
        Err(error) => Err(api_error(error.to_string())),
    }
}

async fn resolve_secrets<T: RuntimeStore>(
    db: &T,
    org_id: Uuid,
    bindings: &BTreeMap<String, Uuid>,
) -> Result<serde_json::Value, String> {
    let cipher = SecretCipher::from_env();
    let mut values = serde_json::Map::new();
    for (name, id) in bindings {
        let record = db
            .fetch_setting_by_id(*id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("secret binding '{name}' does not exist"))?;
        if record.kind != SettingKind::Secret {
            return Err(format!("binding '{name}' does not reference a Secret"));
        }
        let expected_scope = format!("org:{org_id}");
        if record.scope != expected_scope {
            return Err(format!(
                "secret binding '{name}' is outside the adapter organization"
            ));
        }
        let opened = cipher
            .try_decrypt(&record.value)
            .ok_or_else(|| format!("secret binding '{name}' could not be decrypted"))?;
        let secret = StoredSecret::decode(&opened)?;
        if secret
            .expires_at
            .is_some_and(|expires| expires <= Utc::now())
        {
            return Err(format!("secret binding '{name}' is expired"));
        }
        values.insert(name.clone(), serde_json::Value::String(secret.value));
    }
    Ok(serde_json::Value::Object(values))
}

fn webhook_body_limit() -> usize {
    std::env::var("RUNINATOR_ADAPTER_WEBHOOK_BODY_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1024 * 1024)
}

fn allowed_webhook_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    let configured = std::env::var("RUNINATOR_ADAPTER_WEBHOOK_HEADER_ALLOWLIST")
        .unwrap_or_else(|_| {
            "authorization,content-type,x-runinator-signature,x-hub-signature-256,x-github-delivery,x-github-event,x-atlassian-webhook-identifier".into()
        });
    let allowed = configured
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
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

async fn pipeline_for_event<T: DefinitionStore + IngressStore>(
    db: &T,
    adapter: &AdapterDefinition,
    event: &runinator_models::orchestration::NormalizedAdapterEvent,
) -> Result<Uuid, String> {
    if let Some(admission) = db
        .fetch_ingress_admission(
            Some(adapter.org_id),
            event.scope.clone(),
            event.correlation_key.clone(),
        )
        .await
        .map_err(|error| error.to_string())?
    {
        return match admission.target.kind {
            IngressTargetKind::Pipeline => Ok(admission.target.id),
            IngressTargetKind::Workflow => {
                Err("correlation key is owned by a workflow ingress target".into())
            }
        };
    }
    let mut candidates = Vec::new();
    for pipeline in db
        .fetch_pipelines()
        .await
        .map_err(|error| error.to_string())?
    {
        if pipeline.org_id != Some(adapter.org_id) {
            continue;
        }
        let Some(raw_policy) = pipeline.metadata.get("ingress") else {
            continue;
        };
        let policy: IngressPolicy =
            serde_json::from_value(raw_policy.clone().into()).map_err(|error| {
                format!(
                    "pipeline '{}' has invalid ingress policy: {error}",
                    pipeline.name
                )
            })?;
        if policy.scope == event.scope
            && policy.action_for(&event.event_type, IngressLifecycle::Unbound)
                == Some(IngressAction::Start)
        {
            if let Some(id) = pipeline.id {
                candidates.push(id);
            }
        }
    }
    match candidates.as_slice() {
        [pipeline_id] => Ok(*pipeline_id),
        [] => Err(format!(
            "no pipeline admission route matched scope '{}' and event '{}'",
            event.scope, event.event_type
        )),
        _ => Err(format!(
            "multiple pipeline admission routes matched scope '{}' and event '{}'; make admission routes unambiguous",
            event.scope, event.event_type
        )),
    }
}

pub async fn kinds<T: RbacStore>(
    Extension(_db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_scope(&ctx, Action::View) {
        return reply;
    }
    match catalog().await {
        Ok(entries) => {
            let kinds = entries
                .into_iter()
                .filter(|entry| entry.healthy && entry.error.is_none())
                .map(|entry| entry.metadata)
                .collect();
            (StatusCode::OK, Json(ApiResponse::AdapterKindList(kinds)))
        }
        Err(error) => api_error(error),
    }
}

pub async fn list<T: OrchestrationStore + RbacStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    let org_id = match require_scope(&ctx, Action::View) {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    match db.fetch_orchestration_adapters(org_id).await {
        Ok(values) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationAdapterList(values)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn get_one<T: OrchestrationStore + RbacStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match authorized_adapter(db.as_ref(), &ctx, id, Action::View).await {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationAdapter(value)),
        ),
        Err(reply) => reply,
    }
}

pub async fn revisions<T: OrchestrationStore + RbacStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorized_adapter(db.as_ref(), &ctx, id, Action::View).await {
        return reply;
    }
    match db.fetch_orchestration_adapter_revisions(id).await {
        Ok(values) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationAdapterRevisionList(values)),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn create<T: OrchestrationStore + RbacStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<AdapterApplyRequest>,
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
    let now = Utc::now();
    let adapter_id = Uuid::now_v7();
    match db
        .create_orchestration_adapter(
            NewAdapterDefinition {
                id: adapter_id,
                org_id,
                name: request.name,
                kind: request.kind,
                kind_version: request.kind_version,
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
        Ok((adapter, _)) => (
            StatusCode::CREATED,
            Json(ApiResponse::OrchestrationAdapter(adapter)),
        ),
        Err(error) => bad_request(error.to_string()),
    }
}

pub async fn update<T: OrchestrationStore + RbacStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Json(request): Json<AdapterApplyRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let adapter = match authorized_adapter(db.as_ref(), &ctx, id, Action::Edit).await {
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
    let expected_revision = request
        .expected_revision
        .unwrap_or(adapter.current_revision);
    match db
        .create_orchestration_adapter_revision(
            NewAdapterRevision {
                id: Uuid::now_v7(),
                adapter_id: id,
                expected_revision,
                kind_version: request.kind_version,
                configuration: request.configuration,
                secret_bindings: request.secret_bindings,
                identity_configuration: request.identity_configuration,
                actor_id: ctx.principal_id,
            },
            Utc::now(),
        )
        .await
    {
        Ok(Some((adapter, _))) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationAdapter(adapter)),
        ),
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

pub async fn set_enabled<T: OrchestrationStore + RbacStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Json(request): Json<AdapterEnableRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorized_adapter(db.as_ref(), &ctx, id, Action::Edit).await {
        return reply;
    }
    match db
        .set_orchestration_adapter_enabled(id, request.enabled, Utc::now())
        .await
    {
        Ok(Some(adapter)) => (
            StatusCode::OK,
            Json(ApiResponse::OrchestrationAdapter(adapter)),
        ),
        Ok(None) => not_found("adapter not found"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn remove<T: OrchestrationStore + RbacStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorized_adapter(db.as_ref(), &ctx, id, Action::Own).await {
        return reply;
    }
    match db.delete_orchestration_adapter(id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: true,
                message: "adapter deleted".into(),
            })),
        ),
        Ok(false) => {
            bad_request("an adapter that admitted bindings cannot be deleted; disable or clone it")
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn test<T: OrchestrationStore + RbacStore + SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Json(request): Json<AdapterTestRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let adapter = match authorized_adapter(db.as_ref(), &ctx, id, Action::Edit).await {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let revision = match current_revision(db.as_ref(), &adapter).await {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let bindings = request.secret_bindings.unwrap_or(revision.secret_bindings);
    let secrets = match resolve_secrets(db.as_ref(), adapter.org_id, &bindings).await {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
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
    match host_post(
        "/verify-normalize",
        serde_json::json!({ "kind": adapter.kind, "request": adapter_request }),
    )
    .await
    {
        Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value.into()))),
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
    match host_get("/health").await {
        Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value.into()))),
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
    match host_post("/reload", serde_json::json!({})).await {
        Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value.into()))),
        Err(error) => api_error(error),
    }
}

/// Public data-plane endpoint. The selected adapter must authenticate and normalize the request
/// before any event is allowed to reach durable ingress.
pub async fn webhook<
    T: OrchestrationStore
        + SettingStore
        + RuntimeStore
        + DefinitionStore
        + IngressStore
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
    let adapter = match endpoint.parse::<Uuid>() {
        Ok(id) => db.fetch_orchestration_adapter(id).await,
        Err(_) => db.fetch_orchestration_adapter_by_endpoint(endpoint).await,
    };
    let adapter = match adapter {
        Ok(Some(adapter)) if adapter.enabled => adapter,
        Ok(Some(_)) => return not_found("adapter is disabled"),
        Ok(None) => return not_found("adapter not found"),
        Err(error) => return api_error(error.to_string()),
    };
    let revision = match current_revision(db.as_ref(), &adapter).await {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let secrets =
        match resolve_secrets(db.as_ref(), adapter.org_id, &revision.secret_bindings).await {
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
    let normalized: AdapterResponse = match host_post(
        "/verify-normalize",
        serde_json::json!({ "kind": adapter.kind, "request": request }),
    )
    .await
    {
        Ok(value) => match serde_json::from_value(value) {
            Ok(response) => response,
            Err(error) => {
                return api_error(format!("adapter host returned malformed output: {error}"));
            }
        },
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
    for event in normalized.events {
        let pipeline_id = match pipeline_for_event(db.as_ref(), &adapter, &event).await {
            Ok(value) => value,
            Err(error) => return bad_request(error),
        };
        let reply = process_pipeline_ingress(
            db.clone(),
            pipelines.clone(),
            pipeline_id,
            Some(adapter.org_id),
            IngressEventRequest {
                source: format!("adapter:{}:{}", adapter.id, event.source),
                event_id: event.delivery_id,
                event_type: event.event_type,
                correlation_key: event.correlation_key,
                payload: event.payload,
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

pub fn routes<T>(pool: Arc<T>) -> axum::Router
where
    T: OrchestrationStore
        + RbacStore
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
            "/orchestrations/adapters/{id}/enabled",
            post(set_enabled::<T>),
        )
        .route("/orchestrations/adapters/{id}/test", post(test::<T>))
        .route("/webhooks/orchestration/{adapter_id}", post(webhook::<T>))
        .layer(Extension(pool))
}
