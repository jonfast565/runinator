use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path},
    http::{HeaderMap, StatusCode},
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration, Utc};
use runinator_auth::enroll::EnrollToken;
use runinator_models::auth::{
    AddTeamMemberRequest, AgentEnrollmentToken, AgentEnrollmentTokenRecord, ApiKey, ApiKeyRecord,
    AuthContext, AuthSession, AuthSessionSummary, ChangePasswordRequest,
    CreateAgentEnrollmentTokenRequest, CreateAgentEnrollmentTokenResponse, CreateApiKeyRequest,
    CreateApiKeyResponse, CreatePersonalApiKeyRequest, CreateTeamRequest, CreateUserRequest,
    EnrollAgentRequest, EnrollAgentResponse, LoginRequest, LoginResponse, PersonalApiKeyScope,
    PrincipalKind, RefreshRequest, UpdateApiKeyRequest, UpdateCurrentUserRequest,
    UpdateTeamRequest, UpdateUserRequest, User,
};
use runinator_models::rbac::{Action, PlatformRole, Role, ScopeKind, ScopeRef, SystemRole};
use runinator_models::server_settings::{
    RuntimeSettingDefinition, ServerSettings, server_setting_catalog,
};
use runinator_models::validation::{Validate, ValidationError};
use runinator_models::value::Value;
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, OrgStore, RbacStore, SettingStore},
};
use serde::Serialize;
use uuid::Uuid;

use runinator_ws_core::ValidatedJson;
use runinator_ws_core::models::{
    ApiError, ApiResponse, AuthConfigResponseSchema, LoginRequestSchema, LoginResponseSchema,
    RefreshRequestSchema,
};
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::{api_error, bad_request, not_found, task_response_success};
use runinator_ws_middleware::auth::{
    AuthConfig, hash_password, hash_secret, issue_access_token, new_api_key, new_refresh_token,
    verify_password,
};
use runinator_ws_middleware::authz::AuthContextExt;

type Reply = (StatusCode, Json<ApiResponse>);

const ORGANIZATION_MEMBERSHIP_REQUIRED: &str =
    "user must belong to an enabled organization before signing in";

#[derive(Clone)]
struct SessionMetadata {
    created_at: chrono::DateTime<Utc>,
    user_agent: Option<String>,
    ip_address: Option<String>,
}

fn session_metadata(headers: &HeaderMap, addr: SocketAddr) -> SessionMetadata {
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.chars().take(512).collect());
    SessionMetadata {
        created_at: Utc::now(),
        user_agent,
        ip_address: Some(addr.ip().to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct AuthSettingsRequest {
    pub max_refreshes: i64,
}

impl Validate for AuthSettingsRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if !(1..=100_000).contains(&self.max_refreshes) {
            return Err(ValidationError::new(
                "max_refreshes",
                "must be between 1 and 100000",
            ));
        }
        Ok(())
    }
}

async fn max_refreshes<T: AuthStore + RbacStore + RuntimeStore + SettingStore>(
    db: &T,
) -> Result<i64, Reply> {
    runinator_engine::settings::load_server_settings(db)
        .await
        .map(|settings| settings.authentication.max_refreshes as i64)
        .map_err(|err| api_error(err.to_string()))
}

fn unauthorized(message: &str) -> Reply {
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiResponse::ApiError(ApiError::new(message))),
    )
}

fn forbidden(message: &str) -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::ApiError(ApiError::new(message))),
    )
}

fn too_many_requests(retry_after_secs: f64) -> Reply {
    let secs = retry_after_secs.ceil().max(1.0) as u64;
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(ApiResponse::ApiError(ApiError::new(format!(
            "too many login attempts; retry in {secs}s"
        )))),
    )
}

async fn enabled_admin_count<T: AuthStore + RbacStore + RuntimeStore>(
    db: &T,
) -> Result<usize, Reply> {
    let assignments = db
        .list_scope_role_assignments(ScopeRef::PLATFORM)
        .await
        .map_err(|err| api_error(err.to_string()))?;
    let mut count = 0;
    for assignment in assignments {
        if assignment.role == Role::Platform(PlatformRole::Admin) {
            let enabled = match assignment.principal_kind {
                PrincipalKind::User => db
                    .fetch_user(assignment.principal_id)
                    .await
                    .map_err(|err| api_error(err.to_string()))?
                    .is_some_and(|user| !user.disabled),
                PrincipalKind::Service => db
                    .fetch_service_account(assignment.principal_id)
                    .await
                    .map_err(|err| api_error(err.to_string()))?
                    .is_some_and(|account| !account.disabled),
            };
            count += usize::from(enabled);
        }
    }
    Ok(count)
}

async fn would_remove_last_enabled_admin<T: AuthStore + RbacStore + RuntimeStore>(
    db: &T,
    user: &User,
    removes_admin: bool,
) -> Result<bool, Reply> {
    let Some(user_id) = user.id else {
        return Ok(false);
    };
    let is_platform_admin = db
        .list_principal_role_assignments(PrincipalKind::User, user_id)
        .await
        .map_err(|err| api_error(err.to_string()))?
        .iter()
        .any(|assignment| assignment.role == Role::Platform(PlatformRole::Admin));
    if !is_platform_admin || user.disabled || !removes_admin {
        return Ok(false);
    }
    Ok(enabled_admin_count(db).await? <= 1)
}
fn json_value<T: Serialize>(value: &T) -> Result<Value, Reply> {
    serde_json::to_value(value)
        .map(Value::from)
        .map_err(|err| api_error(err.to_string()))
}

fn ok_value<T: Serialize>(value: &T) -> Reply {
    match json_value(value) {
        Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value))),
        Err(reply) => reply,
    }
}

async fn user_with_platform_role<T: AuthStore + RbacStore + RuntimeStore>(
    db: &T,
    user: &User,
) -> Result<Value, Reply> {
    let mut value = serde_json::to_value(user).map_err(|err| api_error(err.to_string()))?;
    let id = user.id.ok_or_else(|| api_error("stored user has no id"))?;
    let role = db
        .list_principal_role_assignments(PrincipalKind::User, id)
        .await
        .map_err(|err| api_error(err.to_string()))?
        .into_iter()
        .filter_map(|assignment| match assignment.role {
            Role::Platform(role) => Some(role),
            _ => None,
        })
        .max()
        .ok_or_else(|| api_error("user has no platform role assignment"))?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "platform_role".to_string(),
            serde_json::to_value(role).unwrap(),
        );
    }
    Ok(Value::from(value))
}

// ---- session helpers ----

async fn has_enabled_organization<T: OrgStore>(db: &T, user_id: Uuid) -> Result<bool, Reply> {
    db.list_user_orgs(user_id)
        .await
        .map(|memberships| memberships.into_iter().any(|(org, _)| !org.disabled))
        .map_err(|err| api_error(err.to_string()))
}

async fn issue_session<T: AuthStore + RbacStore + RuntimeStore + SettingStore + OrgStore>(
    db: &T,
    config: &AuthConfig,
    user: User,
    refresh_count: i64,
    metadata: SessionMetadata,
) -> Result<LoginResponse, Reply> {
    let user_id = user.id.ok_or_else(|| api_error("user is missing an id"))?;
    if !has_enabled_organization(db, user_id).await? {
        return Err(forbidden(ORGANIZATION_MEMBERSHIP_REQUIRED));
    }
    let (refresh_token, refresh_hash) = new_refresh_token();
    let session = AuthSession {
        id: Uuid::new_v4(),
        user_id,
        refresh_token_hash: refresh_hash,
        expires_at: Utc::now() + Duration::seconds(config.refresh_ttl_secs),
        revoked: false,
        refresh_count,
        created_at: metadata.created_at,
        last_seen_at: Utc::now(),
        user_agent: metadata.user_agent,
        ip_address: metadata.ip_address,
    };
    db.create_session(session.clone())
        .await
        .map_err(|err| api_error(err.to_string()))?;
    // login issues an org-less token; the client selects an active org via /auth/switch-org.
    let (access_token, _exp) =
        issue_access_token(config, user_id, session.id, None).map_err(api_error)?;
    let assignments = db
        .list_principal_role_assignments(PrincipalKind::User, user_id)
        .await
        .map_err(|err| api_error(err.to_string()))?;
    let platform_role = assignments
        .iter()
        .filter_map(|assignment| match assignment.role {
            Role::Platform(role) => Some(role),
            _ => None,
        })
        .max();
    let context = AuthContext {
        principal_id: Some(user_id),
        session_id: Some(session.id),
        kind: PrincipalKind::User,
        platform_role,
        assignments: assignments.clone(),
        system_role: None,
        action_ceiling: Vec::new(),
        org_id: None,
    };
    let effective_actions = Action::ALL
        .iter()
        .copied()
        .filter(|action| context.authorize_scope(*action, ScopeRef::PLATFORM))
        .collect();
    Ok(LoginResponse {
        access_token,
        refresh_token,
        expires_in: config.access_ttl_secs,
        user,
        assignments,
        effective_actions,
    })
}

/// public probe so clients can tell whether the API requires authentication.
#[utoipa::path(
    get,
    path = "/auth/config",
    tag = "Auth",
    security(),
    responses((status = 200, description = "whether auth is enforced", body = AuthConfigResponseSchema)),
)]
pub async fn auth_config(Extension(config): Extension<Arc<AuthConfig>>) -> Reply {
    ok_value(&serde_json::json!({ "enabled": config.enabled }))
}

// ---- auth flows ----

/// exchange a username/password for an access + refresh token pair.
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "Auth",
    security(),
    request_body = LoginRequestSchema,
    responses(
        (status = 200, description = "token pair and the authenticated user", body = LoginResponseSchema),
        (status = 401, description = "invalid username or password", body = ApiError),
        (status = 403, description = "user has no enabled organization membership", body = ApiError),
    ),
)]
pub async fn login<T: AuthStore + RbacStore + RuntimeStore + SettingStore + OrgStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(config): Extension<Arc<AuthConfig>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ValidatedJson(request): ValidatedJson<LoginRequest>,
) -> Reply {
    // bound credential brute force per client ip before doing any work.
    if let Err(retry_after) = runinator_ws_middleware::rate_limit::check_login_attempt(addr.ip()) {
        return too_many_requests(retry_after);
    }
    let username = request.username.clone();
    let credential = match db.fetch_local_credential(request.username).await {
        Ok(credential) => credential,
        Err(err) => return api_error(err.to_string()),
    };
    // always perform an argon2 verification so login timing does not reveal whether the username
    // exists. an unknown user verifies against a throwaway hash; the result is discarded below.
    let password_ok = match &credential {
        Some(credential) => verify_password(&request.password, &credential.password_hash),
        None => {
            runinator_auth::dummy_verify(&request.password);
            false
        }
    };
    let Some(credential) = credential else {
        audit_login_failure(db.as_ref(), &username, "unknown user").await;
        return unauthorized("invalid username or password");
    };
    if credential.user.disabled || !password_ok {
        let reason = if credential.user.disabled {
            "account disabled"
        } else {
            "bad password"
        };
        audit_login_failure(db.as_ref(), &username, reason).await;
        return unauthorized("invalid username or password");
    }
    let user_id = credential.user.id;
    if let Some(user_id) = user_id {
        let has_membership = match has_enabled_organization(db.as_ref(), user_id).await {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        if !has_membership {
            audit_login_failure(db.as_ref(), &username, "no enabled organization membership").await;
            return forbidden(ORGANIZATION_MEMBERSHIP_REQUIRED);
        }
    }
    match issue_session(
        db.as_ref(),
        &config,
        credential.user,
        0,
        session_metadata(&headers, addr),
    )
    .await
    {
        Ok(response) => {
            crate::audit::record_audit(
                db.as_ref(),
                user_id,
                "user",
                "auth.login",
                crate::audit::AuditOutcome::Success,
                None,
                None,
                Some(&format!("user {username} logged in")),
            )
            .await;
            ok_value(&response)
        }
        Err(reply) => reply,
    }
}

/// record a failed login attempt without leaking the credential material.
async fn audit_login_failure<T: AuthStore + RbacStore + RuntimeStore>(
    db: &T,
    username: &str,
    reason: &str,
) {
    crate::audit::record_audit(
        db,
        None,
        "anonymous",
        "auth.login",
        crate::audit::AuditOutcome::Failure,
        None,
        None,
        Some(&format!("login failed for '{username}': {reason}")),
    )
    .await;
}

async fn audit_credential_change<T: AuthStore + RbacStore + RuntimeStore>(
    db: &T,
    ctx: &AuthContext,
    action: &str,
    key_id: Uuid,
) {
    crate::audit::record_audit(
        db,
        ctx.principal_id,
        ctx.actor_kind(),
        action,
        crate::audit::AuditOutcome::Success,
        Some("api_key"),
        Some(key_id),
        None,
    )
    .await;
}

#[utoipa::path(
    post,
    path = "/auth/refresh",
    tag = "Auth",
    security(),
    request_body = RefreshRequestSchema,
    responses(
        (status = 200, description = "rotated token pair and authenticated user", body = LoginResponseSchema),
        (status = 401, description = "invalid or expired refresh token", body = ApiError),
        (status = 403, description = "user has no enabled organization membership", body = ApiError),
    ),
)]
pub async fn refresh<T: AuthStore + RbacStore + RuntimeStore + SettingStore + OrgStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(config): Extension<Arc<AuthConfig>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ValidatedJson(request): ValidatedJson<RefreshRequest>,
) -> Reply {
    let hash = hash_secret(&request.refresh_token);
    let session = match db.fetch_session_by_hash(hash).await {
        Ok(Some(session)) => session,
        Ok(None) => return unauthorized("invalid refresh token"),
        Err(err) => return api_error(err.to_string()),
    };
    if session.expires_at < Utc::now() {
        return unauthorized("refresh token expired");
    }
    let max_refreshes = match max_refreshes(db.as_ref()).await {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    if session.refresh_count >= max_refreshes {
        let _ = db.revoke_session(session.id).await;
        return unauthorized("refresh session exhausted");
    }
    let user = match db.fetch_user(session.user_id).await {
        Ok(Some(user)) if !user.disabled => user,
        Ok(_) => return unauthorized("user unavailable"),
        Err(err) => return api_error(err.to_string()),
    };
    // Claim the refresh atomically before minting the replacement. This prevents two concurrent
    // requests presenting the same token from both consuming the session budget.
    match db.consume_session_refresh(session.id, max_refreshes).await {
        Ok(true) => {}
        Ok(false) => return unauthorized("refresh session exhausted or already used"),
        Err(err) => return api_error(err.to_string()),
    }
    let mut metadata = session_metadata(&headers, addr);
    metadata.created_at = session.created_at;
    match issue_session(
        db.as_ref(),
        &config,
        user,
        session.refresh_count + 1,
        metadata,
    )
    .await
    {
        Ok(response) => ok_value(&response),
        Err(reply) => reply,
    }
}

#[utoipa::path(
    get,
    path = "/auth/settings",
    tag = "Auth",
    responses((status = 200, description = "refresh policy", body = serde_json::Value)),
)]
pub async fn auth_settings<T: AuthStore + RbacStore + RuntimeStore + SettingStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if ctx.platform_role != Some(PlatformRole::Admin) {
        return forbidden("only a platform admin may manage refresh policy");
    }
    match max_refreshes(db.as_ref()).await {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "max_refreshes": value,
            }))),
        ),
        Err(reply) => reply,
    }
}

#[utoipa::path(
    put,
    path = "/auth/settings",
    tag = "Auth",
    request_body = serde_json::Value,
    responses((status = 200, description = "refresh policy saved", body = serde_json::Value)),
)]
pub async fn update_auth_settings<T: AuthStore + RbacStore + RuntimeStore + SettingStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<AuthSettingsRequest>,
) -> Reply {
    if ctx.platform_role != Some(PlatformRole::Admin) {
        return forbidden("only a platform admin may manage refresh policy");
    }
    let mut settings = match runinator_engine::settings::load_server_settings(db.as_ref()).await {
        Ok(settings) => settings,
        Err(err) => return api_error(err.to_string()),
    };
    settings.authentication.max_refreshes = match u64::try_from(request.max_refreshes) {
        Ok(value) => value,
        Err(_) => return bad_request("max_refreshes must be positive"),
    };
    if let Err(message) = settings.validate() {
        return bad_request(&message);
    }
    match runinator_engine::settings::save_server_settings(db.as_ref(), &settings).await {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "max_refreshes": request.max_refreshes,
            }))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

#[derive(Serialize)]
struct ServerSettingsResponse {
    values: ServerSettings,
    catalog: Vec<runinator_models::server_settings::ServerSettingDefinition>,
    runtime_catalog: Vec<RuntimeSettingDefinition>,
}

fn runtime_setting_catalog() -> Vec<RuntimeSettingDefinition> {
    fn setting(
        key: &str,
        label: &str,
        description: &str,
        env_name: &str,
        default: &str,
        sensitive: bool,
    ) -> RuntimeSettingDefinition {
        let configured = std::env::var(env_name)
            .ok()
            .filter(|value| !value.is_empty());
        let value = if sensitive {
            if configured.is_some() {
                "configured".into()
            } else {
                "not configured".into()
            }
        } else {
            configured.clone().unwrap_or_else(|| default.into())
        };
        RuntimeSettingDefinition {
            key: key.into(),
            section: "Adapter & Workspace Runtime".into(),
            label: label.into(),
            description: description.into(),
            value,
            source: if configured.is_some() {
                env_name.into()
            } else {
                "default".into()
            },
            restart_required: true,
            sensitive,
        }
    }

    vec![
        setting(
            "runtime.adapter_host_url",
            "Adapter host URL",
            "Loopback URL used by the web service to reach the isolated adapter host.",
            "RUNINATOR_ADAPTER_HOST_URL",
            "http://127.0.0.1:8790",
            false,
        ),
        setting(
            "runtime.adapter_host_token",
            "Adapter host token",
            "Loopback authentication token. Its value is never returned by the settings API.",
            "RUNINATOR_ADAPTER_HOST_TOKEN",
            "not configured",
            true,
        ),
        setting(
            "runtime.adapter_plugin_paths",
            "Adapter plugin paths",
            "Filesystem search paths read by the adapter host during startup and reload.",
            "RUNINATOR_ADAPTER_PLUGIN_PATHS",
            "none (built-ins only)",
            false,
        ),
        setting(
            "runtime.adapter_webhook_body_limit",
            "Webhook body limit",
            "Maximum raw provider request size accepted by the web service, in bytes.",
            "RUNINATOR_ADAPTER_WEBHOOK_BODY_LIMIT",
            "1048576",
            false,
        ),
        setting(
            "runtime.adapter_webhook_header_allowlist",
            "Webhook header allowlist",
            "Only these lower-cased request headers are forwarded to adapter verification.",
            "RUNINATOR_ADAPTER_WEBHOOK_HEADER_ALLOWLIST",
            "authorization,content-type,x-runinator-signature,x-hub-signature-256,x-github-delivery,x-github-event,x-atlassian-webhook-identifier",
            false,
        ),
        setting(
            "runtime.adapter_plugin_timeout_ms",
            "Adapter plugin timeout",
            "Maximum lifetime of one disposable plugin child process, in milliseconds.",
            "RUNINATOR_ADAPTER_PLUGIN_TIMEOUT_MS",
            "5000",
            false,
        ),
        setting(
            "runtime.adapter_body_limit_bytes",
            "Adapter request limit",
            "Maximum serialized request size accepted by the adapter host, in bytes.",
            "RUNINATOR_ADAPTER_BODY_LIMIT_BYTES",
            "1048576",
            false,
        ),
        setting(
            "runtime.adapter_output_limit_bytes",
            "Adapter output limit",
            "Maximum metadata or normalized output accepted from a plugin child, in bytes.",
            "RUNINATOR_ADAPTER_OUTPUT_LIMIT_BYTES",
            "1048576",
            false,
        ),
        setting(
            "runtime.adapter_event_limit",
            "Adapter event limit",
            "Maximum normalized events accepted from one provider delivery.",
            "RUNINATOR_ADAPTER_EVENT_LIMIT",
            "16",
            false,
        ),
        setting(
            "runtime.workspace_root",
            "Worker workspace root",
            "Root beneath which each worker resolves opaque workspace keys. A missing value uses that worker's platform app-data directory.",
            "RUNINATOR_WORKSPACE_ROOT",
            "worker platform default",
            false,
        ),
    ]
}

pub async fn server_settings<T: AuthStore + RbacStore + RuntimeStore + SettingStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if ctx.platform_role != Some(PlatformRole::Admin) {
        return forbidden("only a platform admin may manage server settings");
    }
    match runinator_engine::settings::load_server_settings(db.as_ref()).await {
        Ok(values) => ok_value(&ServerSettingsResponse {
            values,
            catalog: server_setting_catalog(),
            runtime_catalog: runtime_setting_catalog(),
        }),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_server_settings<T: AuthStore + RbacStore + RuntimeStore + SettingStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(settings): ValidatedJson<ServerSettings>,
) -> Reply {
    if ctx.platform_role != Some(PlatformRole::Admin) {
        return forbidden("only a platform admin may manage server settings");
    }
    if let Err(message) = settings.validate() {
        return bad_request(&message);
    }
    match runinator_engine::settings::save_server_settings(db.as_ref(), &settings).await {
        Ok(()) => ok_value(&ServerSettingsResponse {
            values: settings,
            catalog: server_setting_catalog(),
            runtime_catalog: runtime_setting_catalog(),
        }),
        Err(err) => api_error(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "Auth",
    request_body = RefreshRequestSchema,
    responses(
        (status = 200, description = "refresh session revoked", body = runinator_ws_core::models::TaskResponseSchema),
        (status = 401, description = "request is missing or has an invalid credential", body = ApiError),
    ),
)]
pub async fn logout<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    ValidatedJson(request): ValidatedJson<RefreshRequest>,
) -> Reply {
    let hash = hash_secret(&request.refresh_token);
    if let Ok(Some(session)) = db.fetch_session_by_hash(hash).await
        && let Err(err) = db.revoke_session(session.id).await
    {
        return api_error(err.to_string());
    }
    task_response_success("Logged out")
}

/// the principal behind the presented credential (user record, or a service marker).
#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "Auth",
    responses((status = 200, description = "current principal", body = serde_json::Value)),
)]
pub async fn me<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let scope = ctx
        .org_id
        .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
        .unwrap_or(ScopeRef::PLATFORM);
    let effective_actions: Vec<Action> = Action::ALL
        .iter()
        .copied()
        .filter(|action| ctx.authorize_scope(*action, scope))
        .collect();
    let Some(user_id) = ctx.principal_id else {
        return unauthorized("principal missing id");
    };
    if ctx.kind == PrincipalKind::Service {
        return match db.fetch_service_account(user_id).await {
            Ok(Some(account)) => ok_value(&serde_json::json!({
                "principal": account, "principal_kind": "service", "selected_scope": scope,
                "assignments": ctx.assignments, "effective_actions": effective_actions,
                "system_role": ctx.system_role,
            })),
            Ok(None) => not_found("service account not found"),
            Err(err) => api_error(err.to_string()),
        };
    }
    match db.fetch_user(user_id).await {
        Ok(Some(user)) => ok_value(&serde_json::json!({
            "principal": user, "principal_kind": "user", "selected_scope": scope,
            "platform_role": ctx.platform_role, "assignments": ctx.assignments,
            "effective_actions": effective_actions,
        })),
        Ok(None) => not_found("user not found"),
        Err(err) => api_error(err.to_string()),
    }
}
fn current_user_ids(ctx: &AuthContext) -> Result<(Uuid, Uuid), Reply> {
    if ctx.kind != PrincipalKind::User {
        return Err(forbidden("profile management is available only to users"));
    }
    let user_id = ctx
        .principal_id
        .ok_or_else(|| unauthorized("principal missing id"))?;
    let session_id = ctx
        .session_id
        .ok_or_else(|| unauthorized("session missing id"))?;
    Ok((user_id, session_id))
}

async fn audit_profile_event<T: AuthStore + RbacStore + RuntimeStore>(
    db: &T,
    ctx: &AuthContext,
    action: &str,
    outcome: crate::audit::AuditOutcome,
    detail: Option<&str>,
) {
    crate::audit::record_audit(
        db,
        ctx.principal_id,
        ctx.actor_kind(),
        action,
        outcome,
        Some("user"),
        ctx.principal_id,
        detail,
    )
    .await;
}

pub async fn update_current_user<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<UpdateCurrentUserRequest>,
) -> Reply {
    let (user_id, _) = match current_user_ids(&ctx) {
        Ok(ids) => ids,
        Err(reply) => return reply,
    };
    match db.update_user(user_id, request.email, None).await {
        Ok(user) => {
            audit_profile_event(
                db.as_ref(),
                &ctx,
                "profile.update",
                crate::audit::AuditOutcome::Success,
                None,
            )
            .await;
            match user_with_platform_role(db.as_ref(), &user).await {
                Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value))),
                Err(reply) => reply,
            }
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn change_current_password<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<ChangePasswordRequest>,
) -> Reply {
    let (user_id, session_id) = match current_user_ids(&ctx) {
        Ok(ids) => ids,
        Err(reply) => return reply,
    };
    let user = match db.fetch_user(user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return not_found("user not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let credential = match db.fetch_local_credential(user.username).await {
        Ok(Some(credential)) => credential,
        Ok(None) => return unauthorized("current password is incorrect"),
        Err(err) => return api_error(err.to_string()),
    };
    if !verify_password(&request.current_password, &credential.password_hash) {
        audit_profile_event(
            db.as_ref(),
            &ctx,
            "profile.password.change",
            crate::audit::AuditOutcome::Failure,
            Some("current password verification failed"),
        )
        .await;
        return unauthorized("current password is incorrect");
    }
    let hash = match hash_password(&request.new_password) {
        Ok(hash) => hash,
        Err(err) => return api_error(err),
    };
    if let Err(err) = db.set_local_password(user_id, hash).await {
        return api_error(err.to_string());
    }
    if let Err(err) = db.revoke_user_sessions_except(user_id, session_id).await {
        return api_error(err.to_string());
    }
    audit_profile_event(
        db.as_ref(),
        &ctx,
        "profile.password.change",
        crate::audit::AuditOutcome::Success,
        None,
    )
    .await;
    task_response_success("Password changed")
}

pub async fn list_current_sessions<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let (user_id, current_id) = match current_user_ids(&ctx) {
        Ok(ids) => ids,
        Err(reply) => return reply,
    };
    match db.list_user_sessions(user_id, Utc::now()).await {
        Ok(sessions) => {
            let summaries: Vec<AuthSessionSummary> = sessions
                .into_iter()
                .map(|session| AuthSessionSummary {
                    id: session.id,
                    user_agent: session.user_agent,
                    ip_address: session.ip_address,
                    created_at: session.created_at,
                    last_seen_at: session.last_seen_at,
                    expires_at: session.expires_at,
                    current: session.id == current_id,
                })
                .collect();
            match summaries
                .iter()
                .map(json_value)
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
                Err(reply) => reply,
            }
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn revoke_current_session<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(session_id): Path<Uuid>,
) -> Reply {
    let (user_id, _) = match current_user_ids(&ctx) {
        Ok(ids) => ids,
        Err(reply) => return reply,
    };
    let session = match db.fetch_session(session_id).await {
        Ok(Some(session)) if session.user_id == user_id && !session.revoked => session,
        Ok(_) => return not_found("session not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(err) = db.revoke_session(session.id).await {
        return api_error(err.to_string());
    }
    audit_profile_event(
        db.as_ref(),
        &ctx,
        "profile.session.revoke",
        crate::audit::AuditOutcome::Success,
        Some(&session_id.to_string()),
    )
    .await;
    task_response_success("Session revoked")
}

pub async fn revoke_other_sessions<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let (user_id, current_id) = match current_user_ids(&ctx) {
        Ok(ids) => ids,
        Err(reply) => return reply,
    };
    if let Err(err) = db.revoke_user_sessions_except(user_id, current_id).await {
        return api_error(err.to_string());
    }
    audit_profile_event(
        db.as_ref(),
        &ctx,
        "profile.sessions.revoke_others",
        crate::audit::AuditOutcome::Success,
        None,
    )
    .await;
    task_response_success("Other sessions revoked")
}

pub async fn list_personal_api_keys<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let (user_id, _) = match current_user_ids(&ctx) {
        Ok(ids) => ids,
        Err(reply) => return reply,
    };
    match db.list_api_keys(Some(user_id)).await {
        Ok(keys) => match keys.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn personal_api_key_scopes<T: AuthStore + RbacStore + RuntimeStore + OrgStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let (user_id, _) = match current_user_ids(&ctx) {
        Ok(ids) => ids,
        Err(reply) => return reply,
    };
    let mut scopes = vec![PersonalApiKeyScope {
        org_id: None,
        name: "Platform".to_string(),
        actions: Action::ALL
            .iter()
            .copied()
            .filter(|action| ctx.authorize_scope(*action, ScopeRef::PLATFORM))
            .collect(),
    }];
    let memberships = match db.list_user_orgs(user_id).await {
        Ok(memberships) => memberships,
        Err(err) => return api_error(err.to_string()),
    };
    for (org, _) in memberships.into_iter().filter(|(org, _)| !org.disabled) {
        let Some(org_id) = org.id else { continue };
        let scope = ScopeRef::new(ScopeKind::Organization, Some(org_id)).unwrap();
        scopes.push(PersonalApiKeyScope {
            org_id: Some(org_id),
            name: org.name,
            actions: Action::ALL
                .iter()
                .copied()
                .filter(|action| ctx.authorize_scope(*action, scope))
                .collect(),
        });
    }
    match scopes.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
        Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
        Err(reply) => reply,
    }
}

pub async fn create_personal_api_key<T: AuthStore + RbacStore + RuntimeStore + OrgStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CreatePersonalApiKeyRequest>,
) -> Reply {
    let (user_id, _) = match current_user_ids(&ctx) {
        Ok(ids) => ids,
        Err(reply) => return reply,
    };
    let target_scope = request
        .org_id
        .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
        .unwrap_or(ScopeRef::PLATFORM);
    if let Some(org_id) = request.org_id {
        let memberships = match db.list_user_orgs(user_id).await {
            Ok(memberships) => memberships,
            Err(err) => return api_error(err.to_string()),
        };
        if !memberships
            .iter()
            .any(|(org, _)| org.id == Some(org_id) && !org.disabled)
        {
            return forbidden("organization is not available to this user");
        }
    }
    if request
        .action_ceiling
        .iter()
        .any(|action| !ctx.authorize_scope(*action, target_scope))
    {
        return forbidden("api key action ceiling exceeds the caller's authority");
    }
    let generated = new_api_key();
    let key = ApiKey {
        id: Some(Uuid::new_v4()),
        name: request.name,
        principal_kind: PrincipalKind::User,
        principal_id: user_id,
        system_role: None,
        org_id: request.org_id,
        action_ceiling: request.action_ceiling,
        key_prefix: generated.prefix,
        last_used_at: None,
        expires_at: request.expires_at,
        disabled: false,
        created_at: Utc::now(),
    };
    match db
        .create_api_key(ApiKeyRecord {
            key,
            key_hash: generated.key_hash,
        })
        .await
    {
        Ok(stored) => {
            if let Some(id) = stored.id {
                audit_credential_change(db.as_ref(), &ctx, "credential.create", id).await;
            }
            ok_value(&CreateApiKeyResponse {
                api_key: stored,
                secret: generated.secret,
            })
        }
        Err(err) => api_error(err.to_string()),
    }
}

// ---- user administration (admin only) ----

pub async fn list_users<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::MembersManage,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    match db.list_users().await {
        Ok(users) => {
            let mut values = Vec::with_capacity(users.len());
            for user in &users {
                match user_with_platform_role(db.as_ref(), user).await {
                    Ok(value) => values.push(value),
                    Err(reply) => return reply,
                }
            }
            (StatusCode::OK, Json(ApiResponse::JsonList(values)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_user<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CreateUserRequest>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::MembersManage,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    let hash = match hash_password(&request.password) {
        Ok(hash) => hash,
        Err(err) => return api_error(err),
    };
    match db
        .create_user_with_platform_role(
            request.username,
            request.email,
            Some(hash),
            request.platform_role,
            ctx.principal_id,
        )
        .await
    {
        Ok(user) => match user_with_platform_role(db.as_ref(), &user).await {
            Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_user<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(user_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<UpdateUserRequest>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::MembersManage,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    let current = match db.fetch_user(user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return not_found("user not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let demotes_enabled_admin = request
        .platform_role
        .is_some_and(|role| role != PlatformRole::Admin)
        || request.disabled == Some(true);
    match would_remove_last_enabled_admin(db.as_ref(), &current, demotes_enabled_admin).await {
        Ok(true) => return forbidden("cannot remove the last enabled admin user"),
        Ok(false) => {}
        Err(reply) => return reply,
    }
    let password_changed = request.password.is_some();
    if let Some(password) = request.password {
        let hash = match hash_password(&password) {
            Ok(hash) => hash,
            Err(err) => return api_error(err),
        };
        if let Err(err) = db.set_local_password(user_id, hash).await {
            return api_error(err.to_string());
        }
    }
    let next_role = request.platform_role;
    match db
        .update_user(user_id, request.email, request.disabled)
        .await
    {
        Ok(user) => {
            if let Some(role) = next_role
                && let Err(err) = db
                    .upsert_role_assignment(
                        PrincipalKind::User,
                        user_id,
                        ScopeRef::PLATFORM,
                        Role::Platform(role),
                        ctx.principal_id,
                    )
                    .await
            {
                return api_error(err.to_string());
            }
            if (password_changed || user.disabled)
                && let Err(err) = db.revoke_user_sessions(user_id).await
            {
                return api_error(err.to_string());
            }
            match user_with_platform_role(db.as_ref(), &user).await {
                Ok(value) => (StatusCode::OK, Json(ApiResponse::JsonValue(value))),
                Err(reply) => reply,
            }
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_user<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(user_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::MembersManage,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    let current = match db.fetch_user(user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return not_found("user not found"),
        Err(err) => return api_error(err.to_string()),
    };
    match would_remove_last_enabled_admin(db.as_ref(), &current, true).await {
        Ok(true) => return forbidden("cannot delete the last enabled admin user"),
        Ok(false) => {}
        Err(reply) => return reply,
    }
    match db.delete_user(user_id).await {
        Ok(()) => task_response_success("User deleted"),
        Err(err) => api_error(err.to_string()),
    }
}

// ---- api keys ----

pub async fn list_api_keys<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    // admins see every key; everyone else sees only their own.
    let scope = if ctx.is_platform_admin() {
        None
    } else {
        ctx.principal_id
    };
    match db.list_api_keys(scope).await {
        Ok(keys) => match keys.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_api_key<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CreateApiKeyRequest>,
) -> Reply {
    let target_scope = request
        .org_id
        .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
        .or_else(|| {
            (request.principal_kind == PrincipalKind::User)
                .then(|| ScopeRef::new(ScopeKind::User, Some(request.principal_id)).unwrap())
        })
        .unwrap_or(ScopeRef::PLATFORM);
    if let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, target_scope) {
        return reply;
    }
    if request.system_role.is_some() && !ctx.is_platform_admin() {
        return forbidden("only a platform admin may assign a system role");
    }
    match request.principal_kind {
        PrincipalKind::User => match db.fetch_user(request.principal_id).await {
            Ok(Some(user)) if !user.disabled => {}
            Ok(_) => return not_found("user not found"),
            Err(err) => return api_error(err.to_string()),
        },
        PrincipalKind::Service => match db.fetch_service_account(request.principal_id).await {
            Ok(Some(account)) if !account.disabled => {}
            Ok(_) => return not_found("service account not found"),
            Err(err) => return api_error(err.to_string()),
        },
    }
    if let Some(org_id) = request.org_id {
        let assignments = match db
            .list_principal_role_assignments(request.principal_kind, request.principal_id)
            .await
        {
            Ok(assignments) => assignments,
            Err(err) => return api_error(err.to_string()),
        };
        if !assignments.iter().any(|assignment| {
            assignment.scope.kind == ScopeKind::Organization && assignment.scope.id == Some(org_id)
        }) {
            return bad_request("key principal is not assigned to the requested organization");
        }
    }
    if request
        .action_ceiling
        .iter()
        .any(|action| !ctx.authorize_scope(*action, target_scope))
    {
        return forbidden("api key action ceiling exceeds the caller's authority");
    }
    let generated = new_api_key();
    let key = ApiKey {
        id: Some(Uuid::new_v4()),
        name: request.name,
        principal_kind: request.principal_kind,
        principal_id: request.principal_id,
        system_role: request.system_role,
        org_id: request.org_id,
        action_ceiling: request.action_ceiling,
        key_prefix: generated.prefix,
        last_used_at: None,
        expires_at: request.expires_at,
        disabled: false,
        created_at: Utc::now(),
    };
    let record = ApiKeyRecord {
        key,
        key_hash: generated.key_hash,
    };
    match db.create_api_key(record).await {
        Ok(stored) => {
            if let Some(id) = stored.id {
                audit_credential_change(db.as_ref(), &ctx, "credential.create", id).await;
            }
            ok_value(&CreateApiKeyResponse {
                api_key: stored,
                secret: generated.secret,
            })
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_api_key<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(key_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<UpdateApiKeyRequest>,
) -> Reply {
    let current = match db.fetch_api_key(key_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return not_found("api key not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if !can_manage_api_key(&ctx, &current.key) {
        return not_found("api key not found");
    }
    match db
        .update_api_key(key_id, request.name, request.expires_at, request.disabled)
        .await
    {
        Ok(key) => {
            audit_credential_change(db.as_ref(), &ctx, "credential.update", key_id).await;
            ok_value(&key)
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn rotate_api_key<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(key_id): Path<Uuid>,
) -> Reply {
    let current = match db.fetch_api_key(key_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return not_found("api key not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if !can_manage_api_key(&ctx, &current.key) {
        return not_found("api key not found");
    }
    let generated = new_api_key();
    let key = ApiKey {
        id: Some(Uuid::new_v4()),
        name: current.key.name,
        principal_kind: current.key.principal_kind,
        principal_id: current.key.principal_id,
        system_role: current.key.system_role,
        org_id: current.key.org_id,
        action_ceiling: current.key.action_ceiling,
        key_prefix: generated.prefix,
        last_used_at: None,
        expires_at: current.key.expires_at,
        disabled: false,
        created_at: Utc::now(),
    };
    let record = ApiKeyRecord {
        key,
        key_hash: generated.key_hash,
    };
    match db.create_api_key(record).await {
        Ok(stored) => {
            if let Err(err) = db.revoke_api_key(key_id).await {
                return api_error(err.to_string());
            }
            audit_credential_change(db.as_ref(), &ctx, "credential.rotate", key_id).await;
            ok_value(&CreateApiKeyResponse {
                api_key: stored,
                secret: generated.secret,
            })
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn revoke_api_key<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(key_id): Path<Uuid>,
) -> Reply {
    let current = match db.fetch_api_key(key_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return not_found("api key not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if !can_manage_api_key(&ctx, &current.key) {
        return not_found("api key not found");
    }
    match db.revoke_api_key(key_id).await {
        Ok(()) => {
            audit_credential_change(db.as_ref(), &ctx, "credential.revoke", key_id).await;
            task_response_success("API key revoked")
        }
        Err(err) => api_error(err.to_string()),
    }
}

fn can_manage_api_key(ctx: &AuthContext, key: &ApiKey) -> bool {
    if ctx.is_platform_admin() {
        return true;
    }
    if key.principal_kind == ctx.kind && Some(key.principal_id) == ctx.principal_id {
        return true;
    }
    key.org_id
        .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
        .is_some_and(|scope| ctx.authorize_scope(Action::CredentialsManage, scope))
}

// ---- agent enrollment ----

const MAX_ENROLLMENT_TTL_SECONDS: u64 = 86_400;
const ENROLLMENT_REJECTED: &str = "enrollment rejected";

pub async fn create_agent_enrollment_token<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CreateAgentEnrollmentTokenRequest>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::AgentsEnroll,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    if request.ttl_seconds == 0 || request.ttl_seconds > MAX_ENROLLMENT_TTL_SECONDS {
        return bad_request(format!(
            "ttl_seconds must be between 1 and {MAX_ENROLLMENT_TTL_SECONDS}"
        ));
    }
    let Some(service_url) = url::Url::parse(&request.service_url).ok() else {
        return bad_request("service_url must be an http or https URL");
    };
    if !matches!(service_url.scheme(), "http" | "https") {
        return bad_request("service_url must be an http or https URL");
    }
    if let Some(pin) = request.spki_pin.as_deref() {
        let encoded = pin.strip_prefix("sha256/").unwrap_or(pin);
        let valid_pin = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| URL_SAFE_NO_PAD.decode(encoded))
            .is_ok_and(|bytes| bytes.len() == 32);
        if service_url.scheme() != "https" || !valid_pin {
            return bad_request("spki_pin requires an https URL and a base64 SHA-256 digest");
        }
    }
    let mut generated =
        EnrollToken::generate(request.service_url.clone(), request.spki_pin.clone());
    if request.cluster_id.is_some() {
        generated.cluster_id = request.cluster_id;
    }
    let now = Utc::now();
    if let Err(err) = db.purge_expired_enrollment_tokens(now).await {
        log::warn!("failed to purge expired agent enrollment tokens: {err}");
    }
    let token = AgentEnrollmentToken {
        token_id: generated.token_id.clone(),
        org_id: request.org_id,
        labels: request.labels,
        service_url: request.service_url,
        spki_pin: request.spki_pin,
        permanent: request.permanent,
        expires_at: now + Duration::seconds(request.ttl_seconds as i64),
        consumed_at: None,
        issued_by: ctx.principal_id,
        created_at: now,
    };
    let record = AgentEnrollmentTokenRecord {
        token: token.clone(),
        sealed_secret: SecretCipher::from_env().encrypt(&generated.secret),
    };
    match db.create_agent_enrollment_token(record).await {
        Ok(stored) => ok_value(&CreateAgentEnrollmentTokenResponse {
            enrollment_token: stored,
            token: generated.encode(),
        }),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_agent_enrollment_tokens<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::AgentsEnroll,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    match db.list_agent_enrollment_tokens().await {
        Ok(tokens) => match tokens.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_agent_enrollment_token<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(token_id): Path<String>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::AgentsEnroll,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    match db.delete_agent_enrollment_token(token_id).await {
        Ok(()) => task_response_success("Enrollment token deleted"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn enroll_agent<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ValidatedJson(request): ValidatedJson<EnrollAgentRequest>,
) -> Reply {
    if let Err(retry_after) =
        runinator_ws_middleware::rate_limit::check_enrollment_attempt(addr.ip())
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ApiResponse::ApiError(ApiError::new(format!(
                "too many enrollment attempts; retry after {} seconds",
                retry_after.ceil() as u64
            )))),
        );
    }
    match authorize_enrollment(db.as_ref(), request).await {
        Ok(Some(response)) => ok_value(&response),
        Ok(None) => unauthorized(ENROLLMENT_REJECTED),
        Err(err) => {
            log::warn!("agent enrollment failed internally: {err}");
            unauthorized(ENROLLMENT_REJECTED)
        }
    }
}

async fn authorize_enrollment<T: AuthStore + RbacStore + RuntimeStore>(
    db: &T,
    request: EnrollAgentRequest,
) -> Result<Option<EnrollAgentResponse>, runinator_models::errors::SendableError> {
    let Some(stored) = db
        .fetch_agent_enrollment_token(request.token_id.clone())
        .await?
    else {
        return Ok(None);
    };
    let now = Utc::now();
    if stored.token.consumed_at.is_some() || stored.token.expires_at < now {
        return Ok(None);
    }
    let Some(secret) = SecretCipher::from_env().try_decrypt(&stored.sealed_secret) else {
        return Ok(None);
    };
    let canonical = serde_json::to_vec(&request.request_body)?;
    let proof = URL_SAFE_NO_PAD.decode(request.proof).unwrap_or_default();
    let token = EnrollToken {
        token_id: stored.token.token_id.clone(),
        secret,
        service_url: stored.token.service_url.clone(),
        spki_pin: stored.token.spki_pin.clone(),
        cluster_id: None,
    };
    // keep this verification before every authorization-field check: one opaque response and one
    // constant-time HMAC path prevent token-id and scope probing from becoming an oracle.
    if !token.verify_proof(&canonical, &proof)
        || request.request_body.labels.iter().any(|(key, value)| {
            stored
                .token
                .labels
                .get(key)
                .is_none_or(|allowed| allowed != value)
        })
    {
        return Ok(None);
    }

    let generated = new_api_key();
    let service = db
        .create_service_account(
            format!("agent:{}", request.request_body.instance_id),
            stored.token.issued_by,
        )
        .await?;
    let key = ApiKey {
        id: Some(Uuid::new_v4()),
        name: format!("agent:{}", request.request_body.instance_id),
        principal_kind: PrincipalKind::Service,
        principal_id: service.id,
        system_role: Some(SystemRole::Agent),
        org_id: stored.token.org_id,
        action_ceiling: Vec::new(),
        key_prefix: generated.prefix,
        last_used_at: None,
        expires_at: (!stored.token.permanent).then_some(stored.token.expires_at),
        disabled: false,
        created_at: now,
    };
    let record = ApiKeyRecord {
        key,
        key_hash: generated.key_hash,
    };
    let Some(_) = db
        .consume_enrollment_token_and_create_api_key(request.token_id, record, now)
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(EnrollAgentResponse {
        api_key: generated.secret,
        service_url: stored.token.service_url,
        expires_at: (!stored.token.permanent).then_some(stored.token.expires_at),
        org_id: stored.token.org_id,
        labels: request.request_body.labels,
    }))
}

// ---- teams (scoped administration) ----

async fn require_team_management<T: AuthStore + RbacStore + RuntimeStore>(
    db: &T,
    ctx: &AuthContext,
    team_id: Uuid,
) -> Result<(), Reply> {
    let team = match db.fetch_team(team_id).await {
        Ok(Some(team)) => team,
        Ok(None) => return Err(not_found("team not found")),
        Err(err) => return Err(api_error(err.to_string())),
    };
    ctx.require_scope_action(Action::MembersManage, team.scope)
}

pub async fn list_teams<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let scope = ctx.selected_scope();
    if let Err(reply) = ctx.require_scope_action(Action::MembersManage, scope) {
        return reply;
    }
    match db.list_teams().await {
        Ok(teams) => match teams
            .iter()
            .filter(|team| team.scope == scope)
            .map(json_value)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_user_teams<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(user_id): Path<Uuid>,
) -> Reply {
    let scope = ctx.selected_scope();
    if let Err(reply) = ctx.require_scope_action(Action::MembersManage, scope) {
        return reply;
    }
    match db.list_user_teams(user_id).await {
        Ok(teams) => match teams
            .iter()
            .filter(|team| team.scope == scope)
            .map(json_value)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_team<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CreateTeamRequest>,
) -> Reply {
    let scope = ctx.selected_scope();
    if let Err(reply) = ctx.require_scope_action(Action::MembersManage, scope) {
        return reply;
    }
    match db.create_team(request.name, scope).await {
        Ok(team) => ok_value(&team),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_team<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(team_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<UpdateTeamRequest>,
) -> Reply {
    if let Err(reply) = require_team_management(db.as_ref(), &ctx, team_id).await {
        return reply;
    }
    match db.update_team(team_id, request.name).await {
        Ok(team) => ok_value(&team),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_team<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(team_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = require_team_management(db.as_ref(), &ctx, team_id).await {
        return reply;
    }
    match db.delete_team(team_id).await {
        Ok(()) => task_response_success("Team deleted"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_team_members<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(team_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = require_team_management(db.as_ref(), &ctx, team_id).await {
        return reply;
    }
    match db.list_team_members(team_id).await {
        Ok(users) => match users.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn add_team_member<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(team_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<AddTeamMemberRequest>,
) -> Reply {
    if let Err(reply) = require_team_management(db.as_ref(), &ctx, team_id).await {
        return reply;
    }
    match db
        .add_team_member(team_id, request.user_id, request.role)
        .await
    {
        Ok(()) => task_response_success("Member added"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn remove_team_member<T: AuthStore + RbacStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((team_id, user_id)): Path<(Uuid, Uuid)>,
) -> Reply {
    if let Err(reply) = require_team_management(db.as_ref(), &ctx, team_id).await {
        return reply;
    }
    match db.remove_team_member(team_id, user_id).await {
        Ok(()) => task_response_success("Member removed"),
        Err(err) => api_error(err.to_string()),
    }
}

/// the `auth` endpoints.
pub fn routes<T: AuthStore + RbacStore + RuntimeStore + SettingStore + OrgStore>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::Extension;
    use axum::routing::{delete, get, patch, post};
    axum::Router::new()
        .route("/auth/config", get(auth_config))
        .route(
            "/auth/settings",
            get(auth_settings::<T>)
                .put(update_auth_settings::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/server/settings",
            get(server_settings::<T>)
                .put(update_server_settings::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/auth/login",
            post(login::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/refresh",
            post(refresh::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/logout",
            post(logout::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/me",
            get(me::<T>)
                .patch(update_current_user::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/auth/me/password",
            post(change_current_password::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/sessions",
            get(list_current_sessions::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/sessions/revoke-others",
            post(revoke_other_sessions::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/sessions/{id}",
            delete(revoke_current_session::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/me/api-keys",
            get(list_personal_api_keys::<T>)
                .post(create_personal_api_key::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/auth/me/api-key-scopes",
            get(personal_api_key_scopes::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/users",
            get(list_users::<T>)
                .post(create_user::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/users/{id}",
            patch(update_user::<T>)
                .delete(delete_user::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/users/{id}/teams",
            get(list_user_teams::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/api_keys",
            get(list_api_keys::<T>)
                .post(create_api_key::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/api_keys/{id}",
            patch(update_api_key::<T>)
                .delete(revoke_api_key::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/api_keys/{id}/rotate",
            post(rotate_api_key::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/agents/enrollment_tokens",
            get(list_agent_enrollment_tokens::<T>)
                .post(create_agent_enrollment_token::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/agents/enrollment_tokens/{token_id}",
            delete(delete_agent_enrollment_token::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/agents/enroll",
            post(enroll_agent::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/teams",
            get(list_teams::<T>)
                .post(create_team::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/teams/{id}",
            patch(update_team::<T>)
                .delete(delete_team::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/teams/{id}/members",
            get(list_team_members::<T>)
                .post(add_team_member::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/teams/{id}/members/{user_id}",
            delete(remove_team_member::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint!(
        "get",
        "/auth/config",
        "Auth",
        "Read auth configuration",
        "Public endpoint that tells clients whether authentication is enabled.",
        true,
        None,
        &[],
        200,
        "auth configuration",
        Example::AuthConfig,
    ),
    endpoint!(
        "get",
        "/auth/settings",
        "Auth",
        "Read refresh policy",
        "Returns the platform-wide cap for active refresh sessions per user.",
        false,
        None,
        &[],
        200,
        "refresh policy",
        Example::AuthConfig,
    ),
    endpoint!(
        "put",
        "/auth/settings",
        "Auth",
        "Update refresh policy",
        "Sets the platform-wide cap for active refresh sessions per user.",
        false,
        json_body("Refresh policy settings.", Example::None),
        &[],
        200,
        "saved refresh policy",
        Example::AuthConfig,
    ),
    endpoint!(
        "get",
        "/server/settings",
        "Settings",
        "Read server operating policy",
        "Returns current platform-wide operating values and their defaults, valid bounds, usual ranges, units, and descriptions.",
        false,
        None,
        &[],
        200,
        "server settings catalog",
        Example::None,
    ),
    endpoint!(
        "put",
        "/server/settings",
        "Settings",
        "Update server operating policy",
        "Validates and atomically saves the complete platform-wide operating policy.",
        false,
        json_body("Complete server settings document.", Example::None),
        &[],
        200,
        "saved server settings catalog",
        Example::None,
    ),
    endpoint!(
        "post",
        "/auth/login",
        "Auth",
        "Log in",
        "Exchanges a local username and password for an access token and refresh token.",
        true,
        json_body("Username and password.", Example::LoginRequest),
        &[],
        200,
        "token pair",
        Example::LoginResponse,
    ),
    endpoint!(
        "post",
        "/auth/refresh",
        "Auth",
        "Refresh a session",
        "Rotates a refresh token and returns a new access token, refresh token, and user record.",
        true,
        json_body("Refresh token to rotate.", Example::RefreshRequest),
        &[],
        200,
        "rotated token pair",
        Example::LoginResponse,
    ),
    endpoint!(
        "post",
        "/auth/logout",
        "Auth",
        "Log out",
        "Revokes a refresh token. The response is successful even if the token is already gone.",
        false,
        json_body("Refresh token to revoke.", Example::RefreshRequest),
        &[],
        200,
        "refresh session revoked",
        Example::TaskResponse,
    ),
    endpoint!(
        "get",
        "/auth/me",
        "Auth",
        "Get current principal",
        "Returns the current authenticated user, or a service principal marker for service API keys.",
        false,
        None,
        &[],
        200,
        "current principal",
        Example::User,
    ),
    endpoint!(
        "patch",
        "/auth/me",
        "Auth",
        "Update current profile",
        "Updates the authenticated user's self-service profile fields.",
        false,
        json_body("Profile fields to update.", Example::User),
        &[],
        200,
        "updated current user",
        Example::User,
    ),
    endpoint!(
        "post",
        "/auth/me/password",
        "Auth",
        "Change current password",
        "Verifies the current password, changes it, and revokes other sessions.",
        false,
        json_body("Current and replacement passwords.", Example::None),
        &[],
        200,
        "password changed",
        Example::TaskResponse,
    ),
    endpoint!(
        "get",
        "/auth/sessions",
        "Auth",
        "List current user sessions",
        "Lists active refresh sessions owned by the authenticated user.",
        false,
        None,
        &[],
        200,
        "active sessions",
        Example::None,
    ),
    endpoint!(
        "delete",
        "/auth/sessions/{id}",
        "Auth",
        "Revoke a session",
        "Revokes an active refresh session owned by the authenticated user.",
        false,
        None,
        &[],
        200,
        "session revoked",
        Example::TaskResponse,
    ),
    endpoint!(
        "post",
        "/auth/sessions/revoke-others",
        "Auth",
        "Revoke other sessions",
        "Revokes every session owned by the authenticated user except the current one.",
        false,
        None,
        &[],
        200,
        "other sessions revoked",
        Example::TaskResponse,
    ),
    endpoint!(
        "get",
        "/auth/me/api-keys",
        "Auth",
        "List personal API keys",
        "Lists only API keys owned by the authenticated user.",
        false,
        None,
        &[],
        200,
        "personal API keys",
        Example::ApiKeyList,
    ),
    endpoint!(
        "post",
        "/auth/me/api-keys",
        "Auth",
        "Create personal API key",
        "Creates a scoped API key owned by the authenticated user.",
        false,
        json_body("Personal key settings.", Example::None),
        &[],
        200,
        "created personal API key",
        Example::ApiKey,
    ),
    endpoint!(
        "get",
        "/auth/me/api-key-scopes",
        "Auth",
        "List personal API key scopes",
        "Lists organizations and action ceilings available to the authenticated user.",
        false,
        None,
        &[],
        200,
        "personal API key scopes",
        Example::None,
    ),
    endpoint!(
        "get",
        "/users",
        "Auth",
        "List users",
        "Admin endpoint that lists local users.",
        false,
        None,
        &[],
        200,
        "users",
        Example::UserList,
    ),
    endpoint!(
        "post",
        "/users",
        "Auth",
        "Create a user",
        "Admin endpoint that creates a local user and password credential.",
        false,
        json_body("User creation payload.", Example::User),
        &[],
        200,
        "created user",
        Example::User,
    ),
    endpoint!(
        "patch",
        "/users/{id}",
        "Auth",
        "Update a user",
        "Admin endpoint that updates user flags, email, or password.",
        false,
        json_body("User update payload.", Example::User),
        &[],
        200,
        "updated user",
        Example::User,
    ),
    endpoint!(
        "delete",
        "/users/{id}",
        "Auth",
        "Delete a user",
        "Admin endpoint that deletes a local user unless it is the last enabled admin.",
        false,
        None,
        &[],
        200,
        "user deleted",
        Example::TaskResponse,
    ),
    endpoint!(
        "get",
        "/users/{id}/teams",
        "Auth",
        "List user teams",
        "Admin endpoint that lists the teams a user belongs to.",
        false,
        None,
        &[],
        200,
        "user teams",
        Example::Team,
    ),
    endpoint!(
        "get",
        "/api_keys",
        "Auth",
        "List API keys",
        "Lists API keys visible to the caller. Admins see all keys; users see their own keys.",
        false,
        None,
        &[],
        200,
        "api keys",
        Example::ApiKeyList,
    ),
    endpoint!(
        "post",
        "/api_keys",
        "Auth",
        "Create an API key",
        "Creates a personal or, for admins, service API key and returns the secret once.",
        false,
        json_body("API key creation payload.", Example::ApiKey),
        &[],
        200,
        "created api key and secret",
        Example::ApiKey,
    ),
    endpoint!(
        "delete",
        "/api_keys/{id}",
        "Auth",
        "Revoke an API key",
        "Admin endpoint that revokes an API key.",
        false,
        None,
        &[],
        200,
        "api key revoked",
        Example::TaskResponse,
    ),
    endpoint!(
        "patch",
        "/api_keys/{id}",
        "Auth",
        "Update an API key",
        "Admin endpoint that updates API key metadata such as name, expiry, or disabled state.",
        false,
        json_body("API key update payload.", Example::ApiKey),
        &[],
        200,
        "updated api key",
        Example::ApiKeyList,
    ),
    endpoint!(
        "post",
        "/api_keys/{id}/rotate",
        "Auth",
        "Rotate an API key",
        "Admin endpoint that disables an API key and returns a replacement secret once.",
        false,
        None,
        &[],
        200,
        "rotated api key and secret",
        Example::ApiKey,
    ),
    endpoint!(
        "get",
        "/agents/enrollment_tokens",
        "Agents",
        "List agent enrollment tokens",
        "Lists active, consumed, and expired enrollment-token metadata without revealing secrets.",
        false,
        None,
        &[],
        200,
        "agent enrollment tokens",
        Example::AgentEnrollmentTokenList,
    ),
    endpoint!(
        "post",
        "/agents/enrollment_tokens",
        "Agents",
        "Create an agent enrollment token",
        "Creates a TTL-bounded, single-use token scoped to one organization and an allowed label set. Redemption creates timed machine access by default, or non-expiring access when permanent is true. The encoded token is returned only once.",
        false,
        json_body(
            "Enrollment scope, service identity, and lifetime.",
            Example::AgentEnrollmentCreate,
        ),
        &[],
        200,
        "created enrollment token and one-time secret",
        Example::AgentEnrollmentCreate,
    ),
    endpoint!(
        "delete",
        "/agents/enrollment_tokens/{token_id}",
        "Agents",
        "Revoke an agent enrollment token",
        "Revokes an enrollment token before it is redeemed.",
        false,
        None,
        &[],
        200,
        "enrollment token revoked",
        Example::TaskResponse,
    ),
    endpoint!(
        "post",
        "/agents/enroll",
        "Agents",
        "Enroll an agent",
        "Public redemption endpoint authenticated by the token-bound HMAC. Every rejected enrollment returns the same response.",
        true,
        json_body(
            "Agent identity request and token proof.",
            Example::AgentEnrollmentRequest,
        ),
        &[],
        200,
        "issued agent credential",
        Example::AgentEnrollmentResponse,
    ),
    endpoint!(
        "get",
        "/teams",
        "Auth",
        "List teams",
        "Lists teams in the caller's selected platform or organization scope.",
        false,
        None,
        &[],
        200,
        "teams",
        Example::Team,
    ),
    endpoint!(
        "post",
        "/teams",
        "Auth",
        "Create a team",
        "Creates a team in the caller's selected platform or organization scope.",
        false,
        json_body("Team creation payload.", Example::Team),
        &[],
        200,
        "team created",
        Example::Team,
    ),
    endpoint!(
        "delete",
        "/teams/{id}",
        "Auth",
        "Delete a team",
        "Deletes a team in its owning scope after its resources have been transferred.",
        false,
        None,
        &[],
        200,
        "team deleted",
        Example::TaskResponse,
    ),
    endpoint!(
        "patch",
        "/teams/{id}",
        "Auth",
        "Update a team",
        "Renames a team in its owning scope.",
        false,
        json_body("Team update payload.", Example::Team),
        &[],
        200,
        "team updated",
        Example::Team,
    ),
    endpoint!(
        "get",
        "/teams/{id}/members",
        "Auth",
        "List team members",
        "Lists users assigned to a team in the caller's authorized scope.",
        false,
        None,
        &[],
        200,
        "team members",
        Example::UserList,
    ),
    endpoint!(
        "post",
        "/teams/{id}/members",
        "Auth",
        "Add a team member",
        "Adds an enabled user to a team in the caller's authorized scope.",
        false,
        json_body("Team member payload.", Example::Team),
        &[],
        200,
        "member added",
        Example::TaskResponse,
    ),
    endpoint!(
        "delete",
        "/teams/{id}/members/{user_id}",
        "Auth",
        "Remove a team member",
        "Removes a user from a team in the caller's authorized scope.",
        false,
        None,
        &[],
        200,
        "member removed",
        Example::TaskResponse,
    ),
];
