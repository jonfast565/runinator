use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_models::auth::{AuthContext, Permission, ResourceType};
use runinator_models::value::Value;
use runinator_models::{
    server_settings::is_reserved_server_setting, settings::SettingKind, web::TaskResponse,
};
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, SettingStore},
};

use crate::settings::{decode_config_schema, decode_config_value, decode_secret};
use runinator_engine::services::{SettingConfiguration, SettingOperations};
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::models::{
    ApiResponse, CredentialPutRequest, CredentialQuery, SettingMoveRequest,
};
use runinator_ws_core::openapi::docs::{
    CREDENTIAL_QUERY, EndpointDoc, Example, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker};

// the cipher that protects setting values at rest, keyed by `RUNINATOR_CREDENTIAL_KEY` (plus any
// rotation-overlap keys in `RUNINATOR_CREDENTIAL_KEY_PREVIOUS`). the value column holds ciphertext;
// only the web service holds the keys.
fn settings_cipher() -> SecretCipher {
    SecretCipher::from_env()
}

// current time in unix seconds, used to stamp settings that arrive without their own timestamp.
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(serde::Deserialize)]
pub struct RuntimeConsumerQuery {
    pub consumer_run_id: uuid::Uuid,
}

pub async fn get_credential<T: AuthorizationStore + SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<CredentialQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let cipher = settings_cipher();
    if ctx.system_role == Some(runinator_models::rbac::SystemRole::Agent) {
        let (Some(scope), Some(name)) = (query.scope.as_deref(), query.name.as_deref()) else {
            return not_found("credential not found");
        };
        let principal_scope = ctx.principal_id.map(|id| format!("agent:{id}"));
        let org_scope = ctx.org_id.map(|id| format!("org:{id}"));
        if principal_scope.as_deref() != Some(scope) && org_scope.as_deref() != Some(scope) {
            // an agent may read only its own or its enrolled org's explicit scope. use the same
            // response as a missing value so scope probing reveals nothing.
            return not_found("credential not found");
        }
        let _ = name;
    } else if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsRead,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    if query.scope.is_none() && query.name.is_none() {
        let visible = match AuthzChecker::new(db.as_ref(), &ctx)
            .visible_resource_ids(ResourceType::Setting)
            .await
        {
            Ok(ids) => ids,
            Err(reply) => return reply,
        };
        return match db.list_settings(ctx.org_id).await {
            Ok(entries) => (
                StatusCode::OK,
                Json(ApiResponse::JsonList(
                    entries
                        .into_iter()
                        .filter(|entry| visible.as_ref().is_none_or(|ids| ids.contains(&entry.id)))
                        .filter(|entry| {
                            !is_reserved_server_setting(entry.kind, &entry.scope, &entry.name)
                        })
                        .map(|entry| {
                            let expires_at = (entry.kind == SettingKind::Secret)
                                .then(|| cipher.try_decrypt(&entry.value))
                                .flatten()
                                .and_then(|bytes| {
                                    decode_secret(&bytes)
                                        .ok()
                                        .and_then(|secret| secret.expires_at)
                                });
                            runinator_models::json!({
                                "id": entry.id,
                                "org_id": entry.org_id,
                                "scope": entry.scope,
                                "name": entry.name,
                                "kind": entry.kind.as_str(),
                                "expires_at": expires_at,
                            })
                        })
                        .collect(),
                )),
            ),
            Err(err) => api_error(err.to_string()),
        };
    }

    let (Some(scope), Some(name)) = (query.scope, query.name) else {
        return bad_request("credential lookup requires both scope and name");
    };
    if is_reserved_server_setting(query.kind, &scope, &name) {
        return not_found("credential not found");
    }

    match db
        .fetch_setting(ctx.org_id, query.kind, scope.clone(), name.clone())
        .await
    {
        // Config is readable. Human-facing secret reads are metadata-only/write-only.
        Ok(Some(record)) => {
            if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                .require_resource(ResourceType::Setting, record.id, Permission::View)
                .await
            {
                return reply;
            }
            let Some(bytes) = cipher.try_decrypt(&record.value) else {
                return api_error(
                    "stored credential could not be decrypted; the encryption key may be unavailable",
                );
            };
            let (value, schema, expires_at) = match query.kind {
                SettingKind::Config => (
                    decode_config_value(&bytes),
                    decode_config_schema(&bytes),
                    None,
                ),
                SettingKind::Secret => {
                    let secret = match decode_secret(&bytes) {
                        Ok(secret) => secret,
                        Err(_) => {
                            return api_error("stored credential has an invalid secret envelope");
                        }
                    };
                    (Value::Null, None, secret.expires_at)
                }
            };
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(runinator_models::json!({
                    "id": record.id,
                    "org_id": record.org_id,
                    "scope": scope,
                    "name": name,
                    "kind": query.kind.as_str(),
                    "value": value,
                    "schema": schema,
                    "expires_at": expires_at,
                }))),
            )
        }
        Ok(None) => not_found("credential not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_credential_by_id<T: AuthorizationStore + SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(setting_id): Path<uuid::Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let record = match db.fetch_setting_by_id(ctx.org_id, setting_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return not_found("credential not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if record.org_id != ctx.org_id || ctx.system_role.is_some() {
        return not_found("credential not found");
    }
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsRead,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Setting, setting_id, Permission::View)
        .await
    {
        return reply;
    }
    if record.kind != SettingKind::Secret {
        return not_found("credential not found");
    }
    let expires_at = settings_cipher()
        .try_decrypt(&record.value)
        .and_then(|bytes| decode_secret(&bytes).ok())
        .and_then(|secret| secret.expires_at);
    (
        StatusCode::OK,
        Json(ApiResponse::JsonValue(runinator_models::json!({
            "id": record.id,
            "org_id": record.org_id,
            "scope": record.scope,
            "name": record.name,
            "kind": record.kind.as_str(),
            "expires_at": expires_at,
            "write_only": true,
        }))),
    )
}

/// Runtime-only secret materialization. Only authenticated worker/agent system principals can
/// resolve plaintext, and the row must belong to their organization.
pub async fn resolve_runtime_secret<T: SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(setting_id): Path<uuid::Uuid>,
    Query(query): Query<RuntimeConsumerQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if !matches!(
        ctx.system_role,
        Some(
            runinator_models::rbac::SystemRole::Worker | runinator_models::rbac::SystemRole::Agent
        )
    ) {
        return not_found("credential not found");
    }
    let admitted = SettingOperations::new(db.clone())
        .run_admitted_setting(query.consumer_run_id, setting_id)
        .await
        .unwrap_or(false);
    if !admitted {
        return not_found("credential not found");
    }
    let record = match db.fetch_setting_by_id(ctx.org_id, setting_id).await {
        Ok(Some(record)) if record.org_id == ctx.org_id && record.kind == SettingKind::Secret => {
            record
        }
        Ok(_) => return not_found("credential not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let secret = settings_cipher()
        .try_decrypt(&record.value)
        .and_then(|bytes| decode_secret(&bytes).ok());
    match secret {
        Some(secret)
            if secret
                .expires_at
                .is_some_and(|expires_at| expires_at <= chrono::Utc::now()) =>
        {
            bad_request("secret has expired")
        }
        Some(secret) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "id": record.id,
                "value": secret.value,
                "expires_at": secret.expires_at,
            }))),
        ),
        None => api_error("stored credential could not be decrypted"),
    }
}

pub async fn put_credential<T: AuthorizationStore + SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CredentialPutRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsWrite,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    if is_reserved_server_setting(request.kind, &request.scope, &request.name) {
        return bad_request(
            "server/operational_policy must be changed through the server settings endpoint",
        );
    }
    if ctx.org_id.is_none() {
        return bad_request("settings must be created inside an organization");
    }
    let existing = match db
        .fetch_setting(
            ctx.org_id,
            request.kind,
            request.scope.clone(),
            request.name.clone(),
        )
        .await
    {
        Ok(record) => record,
        Err(err) => return api_error(err.to_string()),
    };
    if let Some(record) = &existing
        && let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_resource(ResourceType::Setting, record.id, Permission::Edit)
            .await
    {
        return reply;
    }
    match SettingOperations::new(db.clone())
        .configure(SettingConfiguration {
            org_id: ctx.org_id,
            kind: request.kind,
            scope: request.scope.clone(),
            name: request.name.clone(),
            value: request.value.clone(),
            schema: request.schema.clone(),
            expires_at: request.expires_at,
        })
        .await
    {
        Ok(_) => {
            if existing.is_none() {
                let created = match db
                    .fetch_setting(
                        ctx.org_id,
                        request.kind,
                        request.scope.clone(),
                        request.name.clone(),
                    )
                    .await
                {
                    Ok(Some(record)) => record,
                    Ok(None) => return api_error("created setting was not found"),
                    Err(err) => return api_error(err.to_string()),
                };
                if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                    .grant_resource_owner(ResourceType::Setting, created.id)
                    .await
                {
                    return reply;
                }
            }
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(runinator_models::json!({
                    "scope": request.scope,
                    "name": request.name,
                    "kind": request.kind.as_str(),
                    "expires_at": request.expires_at,
                    "stored": true
                }))),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// Re-encrypt stored settings sealed by a secondary key with the current primary key. Run it while
/// the old key is still configured as a secondary, then retire that key. Values already sealed by
/// the primary key are left untouched.
pub async fn reencrypt_settings<T: AuthorizationStore + SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsWrite,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    let cipher = settings_cipher();
    let entries = match db.list_settings(ctx.org_id).await {
        Ok(entries) => entries,
        Err(err) => return api_error(err.to_string()),
    };
    let mut rewritten = 0usize;
    let mut skipped = 0usize;
    for entry in entries {
        if AuthzChecker::new(db.as_ref(), &ctx)
            .require_resource(ResourceType::Setting, entry.id, Permission::Edit)
            .await
            .is_err()
        {
            skipped += 1;
            continue;
        }
        // values already sealed by the primary key need no work.
        if !cipher.needs_reencrypt(&entry.value) {
            continue;
        }
        // never clobber a value we cannot open with the configured keys.
        let Some(plaintext) = cipher.try_decrypt(&entry.value) else {
            skipped += 1;
            continue;
        };
        if let Err(err) = db
            .upsert_setting(
                ctx.org_id,
                entry.kind,
                entry.scope.clone(),
                entry.name.clone(),
                cipher.encrypt(&plaintext),
                now_unix(),
            )
            .await
        {
            return api_error(err.to_string());
        }
        rewritten += 1;
    }
    (
        StatusCode::OK,
        Json(ApiResponse::JsonValue(runinator_models::json!({
            "reencrypted": rewritten,
            "skipped": skipped
        }))),
    )
}

pub async fn delete_credential<
    T: AuthorizationStore + DefinitionStore + SettingStore + RuntimeStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<CredentialQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsWrite,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    let (Some(scope), Some(name)) = (query.scope, query.name) else {
        return bad_request("credential deletion requires both scope and name");
    };
    if is_reserved_server_setting(query.kind, &scope, &name) {
        return bad_request("server/operational_policy cannot be deleted through credentials");
    }
    let record = match db
        .fetch_setting(ctx.org_id, query.kind, scope.clone(), name.clone())
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) => return not_found("credential not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Setting, record.id, Permission::Edit)
        .await
    {
        return reply;
    }

    match SettingOperations::new(db)
        .delete(ctx.org_id, query.kind, scope, name)
        .await
    {
        Ok(inbound) if !inbound.is_empty() => {
            bad_request(format!("setting is referenced by {}", inbound.join(", ")))
        }
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: true,
                message: "Credential deleted".into(),
            })),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn move_credential<
    T: AuthorizationStore + DefinitionStore + SettingStore + RuntimeStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(setting_id): Path<uuid::Uuid>,
    ValidatedJson(request): ValidatedJson<SettingMoveRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsWrite,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    if request.scope.trim().is_empty() || request.name.trim().is_empty() {
        return bad_request("setting scope and name must not be empty");
    }
    let existing = match db.fetch_setting_by_id(ctx.org_id, setting_id).await {
        Ok(Some(record)) if record.org_id == ctx.org_id => record,
        Ok(Some(_)) => return not_found("setting not found"),
        Ok(None) => return not_found("setting not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::Setting, setting_id, Permission::Edit)
        .await
    {
        return reply;
    }
    if existing.kind != request.kind {
        return bad_request("a setting move cannot change its kind");
    }
    if is_reserved_server_setting(existing.kind, &existing.scope, &existing.name)
        || is_reserved_server_setting(request.kind, &request.scope, &request.name)
    {
        return bad_request("server/operational_policy cannot be moved through credentials");
    }
    match SettingOperations::new(db)
        .move_setting(
            setting_id,
            ctx.org_id,
            existing.kind,
            request.scope,
            request.name,
        )
        .await
    {
        Ok(Some(record)) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "id": record.id,
                "kind": record.kind.as_str(),
                "scope": record.scope,
                "name": record.name,
            }))),
        ),
        Ok(None) => not_found("setting not found"),
        Err(err) => bad_request(err.to_string()),
    }
}

/// the `credentials` endpoints.
pub fn routes<T: AuthorizationStore + DefinitionStore + SettingStore + RuntimeStore>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            "/credentials",
            get(get_credential::<T>)
                .post(put_credential::<T>)
                .delete(delete_credential::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/credentials/{id}",
            get(get_credential_by_id::<T>)
                .patch(move_credential::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/credentials/reencrypt",
            post(reencrypt_settings::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/runtime/secrets/{id}",
            get(resolve_runtime_secret::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/credentials",
        "Credentials",
        "Get credentials or config",
        "Fetches a credential/config entry or lists entries by scope/kind. Secret values remain protected by the credential store behavior.",
        false,
        None,
        CREDENTIAL_QUERY,
        200,
        "credential metadata",
        Example::Credential,
    ),
    endpoint(
        "post",
        "/credentials",
        "Credentials",
        "Store a credential or config value",
        "Stores a secret or typed config value. Config values carry or infer a JSON schema pinned for future updates.",
        false,
        json_body("Credential or config value.", Example::Credential),
        &[],
        200,
        "credential stored",
        Example::Credential,
    ),
    endpoint(
        "delete",
        "/credentials",
        "Credentials",
        "Delete a credential or config value",
        "Deletes a secret or config setting selected by query parameters.",
        false,
        None,
        CREDENTIAL_QUERY,
        200,
        "credential deleted",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/credentials/{id}",
        "Credentials",
        "Get secret metadata by UUID",
        "Returns scoped metadata for a write-only secret without materializing its plaintext value.",
        false,
        None,
        &[],
        200,
        "write-only secret metadata",
        Example::Credential,
    ),
    endpoint(
        "patch",
        "/credentials/{id}",
        "Credentials",
        "Move or rename a setting",
        "Changes a setting alias while preserving its durable UUID and workflow references.",
        false,
        json_body("New setting kind, scope, and name.", Example::Credential),
        &[],
        200,
        "setting moved",
        Example::Credential,
    ),
    endpoint(
        "get",
        "/runtime/secrets/{id}",
        "Credentials",
        "Resolve runtime secret plaintext",
        "System-role-only endpoint used by workers and agents to materialize a scoped secret by UUID.",
        false,
        None,
        &[],
        200,
        "runtime secret value",
        Example::Credential,
    ),
    endpoint(
        "post",
        "/credentials/reencrypt",
        "Credentials",
        "Re-encrypt stored settings",
        "Re-encrypts stored secrets/config values after credential-store rotation.",
        false,
        None,
        &[],
        200,
        "settings re-encrypted",
        Example::TaskResponse,
    ),
];
