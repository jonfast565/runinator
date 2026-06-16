use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use chrono::{Duration, Utc};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::{
    AddTeamMemberRequest, ApiKey, ApiKeyRecord, AuthContext, AuthSession, CreateApiKeyRequest,
    CreateApiKeyResponse, CreateGrantRequest, CreateTeamRequest, CreateUserRequest, Grant,
    LoginRequest, LoginResponse, Permission, RefreshRequest, ResourceType, UpdateUserRequest, User,
};
use runinator_models::value::Value;
use serde::Serialize;
use uuid::Uuid;

use crate::auth::{
    AuthConfig, hash_password, hash_secret, issue_access_token, new_api_key, new_refresh_token,
    verify_password,
};
use crate::authz;
use crate::models::{
    ApiError, ApiResponse, AuthConfigResponseSchema, LoginRequestSchema, LoginResponseSchema,
    RefreshRequestSchema,
};
use crate::responses::{api_error, not_found, task_response_success};

type Reply = (StatusCode, Json<ApiResponse>);

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

fn require_admin(ctx: &AuthContext) -> Result<(), Reply> {
    if ctx.is_admin {
        Ok(())
    } else {
        Err(forbidden("admin privileges required"))
    }
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

// ---- session helpers ----

async fn issue_session<T: DatabaseImpl>(
    db: &T,
    config: &AuthConfig,
    user: User,
) -> Result<LoginResponse, Reply> {
    let user_id = user.id.ok_or_else(|| api_error("user is missing an id"))?;
    let (access_token, _exp) =
        issue_access_token(config, user_id, user.is_admin).map_err(|err| api_error(err))?;
    let (refresh_token, refresh_hash) = new_refresh_token();
    let session = AuthSession {
        id: Uuid::new_v4(),
        user_id,
        refresh_token_hash: refresh_hash,
        expires_at: Utc::now() + Duration::seconds(config.refresh_ttl_secs),
        revoked: false,
    };
    db.create_session(session)
        .await
        .map_err(|err| api_error(err.to_string()))?;
    Ok(LoginResponse {
        access_token,
        refresh_token,
        expires_in: config.access_ttl_secs,
        user,
    })
}

/// public probe so clients can tell whether the api requires authentication.
#[utoipa::path(
    get,
    path = "/auth/config",
    tag = "Auth",
    security(),
    responses((status = 200, description = "whether auth is enforced", body = AuthConfigResponseSchema)),
)]
pub(crate) async fn auth_config(Extension(config): Extension<Arc<AuthConfig>>) -> Reply {
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
    ),
)]
pub(crate) async fn login<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(config): Extension<Arc<AuthConfig>>,
    Json(request): Json<LoginRequest>,
) -> Reply {
    let credential = match db.fetch_local_credential(request.username).await {
        Ok(Some(credential)) => credential,
        Ok(None) => return unauthorized("invalid username or password"),
        Err(err) => return api_error(err.to_string()),
    };
    if credential.user.disabled || !verify_password(&request.password, &credential.password_hash) {
        return unauthorized("invalid username or password");
    }
    match issue_session(db.as_ref(), &config, credential.user).await {
        Ok(response) => ok_value(&response),
        Err(reply) => reply,
    }
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
    ),
)]
pub(crate) async fn refresh<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(config): Extension<Arc<AuthConfig>>,
    Json(request): Json<RefreshRequest>,
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
    let user = match db.fetch_user(session.user_id).await {
        Ok(Some(user)) if !user.disabled => user,
        Ok(_) => return unauthorized("user unavailable"),
        Err(err) => return api_error(err.to_string()),
    };
    // rotate: revoke the presented session and mint a fresh one.
    if let Err(err) = db.revoke_session(session.id).await {
        return api_error(err.to_string());
    }
    match issue_session(db.as_ref(), &config, user).await {
        Ok(response) => ok_value(&response),
        Err(reply) => reply,
    }
}

#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "Auth",
    request_body = RefreshRequestSchema,
    responses(
        (status = 200, description = "refresh session revoked", body = crate::models::TaskResponseSchema),
        (status = 401, description = "request is missing or has an invalid credential", body = ApiError),
    ),
)]
pub(crate) async fn logout<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(request): Json<RefreshRequest>,
) -> Reply {
    let hash = hash_secret(&request.refresh_token);
    if let Ok(Some(session)) = db.fetch_session_by_hash(hash).await {
        if let Err(err) = db.revoke_session(session.id).await {
            return api_error(err.to_string());
        }
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
pub(crate) async fn me<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let Some(user_id) = ctx.principal_id else {
        return ok_value(&serde_json::json!({ "service": true, "is_admin": ctx.is_admin }));
    };
    match db.fetch_user(user_id).await {
        Ok(Some(user)) => ok_value(&user),
        Ok(None) => not_found("user not found"),
        Err(err) => api_error(err.to_string()),
    }
}

// ---- user administration (admin only) ----

pub(crate) async fn list_users<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    match db.list_users().await {
        Ok(users) => match users.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_user<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<CreateUserRequest>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    let hash = match hash_password(&request.password) {
        Ok(hash) => hash,
        Err(err) => return api_error(err),
    };
    match db
        .create_user(
            request.username,
            request.email,
            request.is_admin,
            Some(hash),
        )
        .await
    {
        Ok(user) => ok_value(&user),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn update_user<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<UpdateUserRequest>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    if let Some(password) = request.password {
        let hash = match hash_password(&password) {
            Ok(hash) => hash,
            Err(err) => return api_error(err),
        };
        if let Err(err) = db.set_local_password(user_id, hash).await {
            return api_error(err.to_string());
        }
    }
    match db
        .update_user(user_id, request.email, request.is_admin, request.disabled)
        .await
    {
        Ok(user) => ok_value(&user),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_user<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(user_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    match db.delete_user(user_id).await {
        Ok(()) => task_response_success("User deleted"),
        Err(err) => api_error(err.to_string()),
    }
}

// ---- api keys ----

pub(crate) async fn list_api_keys<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    // admins see every key; everyone else sees only their own.
    let scope = if ctx.is_admin { None } else { ctx.principal_id };
    match db.list_api_keys(scope).await {
        Ok(keys) => match keys.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_api_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Reply {
    // only admins may mint service (account-less, trusted) keys; others get a personal key.
    let is_service = request.is_service && ctx.is_admin;
    let user_id = if is_service { None } else { ctx.principal_id };
    let is_admin = if is_service { true } else { ctx.is_admin };
    let generated = new_api_key();
    let key = ApiKey {
        id: Some(Uuid::new_v4()),
        name: request.name,
        user_id,
        is_service,
        key_prefix: generated.prefix,
        last_used_at: None,
        expires_at: request.expires_at,
        disabled: false,
        created_at: Utc::now(),
    };
    let record = ApiKeyRecord {
        key,
        is_admin,
        key_hash: generated.key_hash,
    };
    match db.create_api_key(record).await {
        Ok(stored) => ok_value(&CreateApiKeyResponse {
            api_key: stored,
            secret: generated.secret,
        }),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn revoke_api_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(key_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    match db.revoke_api_key(key_id).await {
        Ok(()) => task_response_success("API key revoked"),
        Err(err) => api_error(err.to_string()),
    }
}

// ---- resource grants (sharing) ----

pub(crate) async fn list_workflow_grants<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
) -> Reply {
    // only an owner (or admin) may inspect/manage a workflow's sharing.
    if let Err(reply) =
        authz::require_workflow(db.as_ref(), &ctx, workflow_id, Permission::Own).await
    {
        return reply;
    }
    match db
        .list_grants(ResourceType::Workflow.as_str().into(), workflow_id)
        .await
    {
        Ok(grants) => match grants.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_workflow_grant<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_id): Path<Uuid>,
    Json(request): Json<CreateGrantRequest>,
) -> Reply {
    if let Err(reply) =
        authz::require_workflow(db.as_ref(), &ctx, workflow_id, Permission::Own).await
    {
        return reply;
    }
    let grant = Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type: request.principal_type,
        principal_id: request.principal_id,
        permission: request.permission,
        created_at: Utc::now(),
    };
    match db.create_grant(grant).await {
        Ok(stored) => ok_value(&stored),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn revoke_workflow_grant<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((workflow_id, grant_id)): Path<(Uuid, Uuid)>,
) -> Reply {
    if let Err(reply) =
        authz::require_workflow(db.as_ref(), &ctx, workflow_id, Permission::Own).await
    {
        return reply;
    }
    match db.revoke_grant(grant_id).await {
        Ok(()) => task_response_success("Grant revoked"),
        Err(err) => api_error(err.to_string()),
    }
}

// ---- teams (admin only) ----

pub(crate) async fn list_teams<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    match db.list_teams().await {
        Ok(teams) => match teams.iter().map(json_value).collect::<Result<Vec<_>, _>>() {
            Ok(values) => (StatusCode::OK, Json(ApiResponse::JsonList(values))),
            Err(reply) => reply,
        },
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_team<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<CreateTeamRequest>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    match db.create_team(request.name).await {
        Ok(team) => ok_value(&team),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_team<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(team_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    match db.delete_team(team_id).await {
        Ok(()) => task_response_success("Team deleted"),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn add_team_member<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(team_id): Path<Uuid>,
    Json(request): Json<AddTeamMemberRequest>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    match db.add_team_member(team_id, request.user_id).await {
        Ok(()) => task_response_success("Member added"),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn remove_team_member<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((team_id, user_id)): Path<(Uuid, Uuid)>,
) -> Reply {
    if let Err(reply) = require_admin(&ctx) {
        return reply;
    }
    match db.remove_team_member(team_id, user_id).await {
        Ok(()) => task_response_success("Member removed"),
        Err(err) => api_error(err.to_string()),
    }
}
