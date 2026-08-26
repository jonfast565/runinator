use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_models::auth::AuthContext;
use runinator_models::value::Value;
use runinator_models::{
    bundles::{SecretBundle, SecretBundleEntry},
    settings::{SettingBinding, SettingKind},
    web::TaskResponse,
};
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, SettingStore},
};

use crate::settings::{
    decode_config_schema, decode_config_value, decode_secret, validate_and_encode_with_expiry,
};
use runinator_ws_core::models::{
    ApiResponse, CredentialPutRequest, CredentialQuery, SettingMoveRequest,
};
use runinator_ws_core::openapi::docs::{
    CREDENTIAL_QUERY, EndpointDoc, Example, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::AuthContextExt;

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

pub async fn get_credential<T: SettingStore + RuntimeStore>(
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
        return match db.list_settings().await {
            Ok(entries) => (
                StatusCode::OK,
                Json(ApiResponse::JsonList(
                    entries
                        .into_iter()
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

    match db
        .fetch_setting(query.kind, scope.clone(), name.clone())
        .await
    {
        // config is non-sensitive: return the parsed json value. secrets return the raw string.
        Ok(Some(record)) => {
            let Some(bytes) = cipher.try_decrypt(&record.value) else {
                return api_error(
                    "stored credential could not be decrypted; the encryption key may be unavailable",
                );
            };
            let (value, expires_at) = match query.kind {
                SettingKind::Config => (decode_config_value(&bytes), None),
                SettingKind::Secret => {
                    let secret = match decode_secret(&bytes) {
                        Ok(secret) => secret,
                        Err(_) => {
                            return api_error("stored credential has an invalid secret envelope");
                        }
                    };
                    (Value::String(secret.value), secret.expires_at)
                }
            };
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(runinator_models::json!({
                    "scope": scope,
                    "name": name,
                    "kind": query.kind.as_str(),
                    "value": value,
                    "expires_at": expires_at,
                }))),
            )
        }
        Ok(None) => not_found("credential not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_credential_by_id<T: SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(setting_id): Path<uuid::Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let record = match db.fetch_setting_by_id(setting_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return not_found("credential not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if ctx.system_role == Some(runinator_models::rbac::SystemRole::Agent) {
        let principal_scope = ctx.principal_id.map(|id| format!("agent:{id}"));
        let org_scope = ctx.org_id.map(|id| format!("org:{id}"));
        if principal_scope.as_deref() != Some(record.scope.as_str())
            && org_scope.as_deref() != Some(record.scope.as_str())
        {
            return not_found("credential not found");
        }
    } else if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsRead,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    if record.kind != SettingKind::Secret {
        return not_found("credential not found");
    }
    let Some(bytes) = settings_cipher().try_decrypt(&record.value) else {
        return api_error(
            "stored credential could not be decrypted; the encryption key may be unavailable",
        );
    };
    let secret = match decode_secret(&bytes) {
        Ok(secret) => secret,
        Err(_) => return api_error("stored credential has an invalid secret envelope"),
    };
    (
        StatusCode::OK,
        Json(ApiResponse::JsonValue(runinator_models::json!({
            "id": record.id,
            "scope": record.scope,
            "name": record.name,
            "kind": record.kind.as_str(),
            "expires_at": secret.expires_at,
            "value": secret.value,
        }))),
    )
}

pub async fn put_credential<T: SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<CredentialPutRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsWrite,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    let cipher = settings_cipher();
    // reuse the schema pinned by a prior write of this config slot, if any.
    let stored_schema = match config_stored_schema(
        db.as_ref(),
        &cipher,
        request.kind,
        &request.scope,
        &request.name,
    )
    .await
    {
        Ok(schema) => schema,
        Err(err) => return api_error(err),
    };
    let bytes = match validate_and_encode_with_expiry(
        request.kind,
        &request.scope,
        &request.name,
        &request.value,
        request.schema.as_ref(),
        stored_schema.as_ref(),
        request.expires_at,
    ) {
        Ok(bytes) => bytes,
        Err(message) => return bad_request(message),
    };
    let ciphertext = cipher.encrypt(&bytes);
    match db
        .upsert_setting(
            request.kind,
            request.scope.clone(),
            request.name.clone(),
            ciphertext,
            now_unix(),
        )
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "scope": request.scope,
                "name": request.name,
                "kind": request.kind.as_str(),
                "expires_at": request.expires_at,
                "stored": true
            }))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// Re-encrypt stored settings sealed by a secondary key with the current primary key. Run it while
/// the old key is still configured as a secondary, then retire that key. Values already sealed by
/// the primary key are left untouched.
pub async fn reencrypt_settings<T: SettingStore + RuntimeStore>(
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
    let entries = match db.list_settings().await {
        Ok(entries) => entries,
        Err(err) => return api_error(err.to_string()),
    };
    let mut rewritten = 0usize;
    let mut skipped = 0usize;
    for entry in entries {
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

/// a secret-import failure tagged with whether it is a client (bad request) or server error.
pub struct SecretImportError {
    bad_request: bool,
    message: String,
}

impl SecretImportError {
    pub fn into_response(self) -> (StatusCode, Json<ApiResponse>) {
        if self.bad_request {
            bad_request(self.message)
        } else {
            api_error(self.message)
        }
    }
}

/// Import every entry in a secret bundle into the settings store, reconciling by modification time,
/// and return the redacted echo. Used by the compiled pack import at `/packs/import`.
pub async fn import_secret_entries<T: SettingStore + RuntimeStore>(
    db: &T,
    bundle: &SecretBundle,
) -> Result<Vec<SecretBundleEntry>, SecretImportError> {
    import_secret_entries_with(db, bundle, false).await
}

// `overwrite` makes an explicit re-apply authoritative: an existing setting is replaced even when
// the incoming entry is not strictly newer, bypassing the reconciliation timestamp gate.
pub async fn import_secret_entries_with<T: SettingStore + RuntimeStore>(
    db: &T,
    bundle: &SecretBundle,
    overwrite: bool,
) -> Result<Vec<SecretBundleEntry>, SecretImportError> {
    runinator_engine::settings::import_setting_bundle_with(db, bundle, overwrite)
        .await
        .map_err(|error| SecretImportError {
            bad_request: error.bad_request,
            message: error.message,
        })
}

// the schema pinned in a config slot's previously-stored bytes, if any. secrets carry no schema.
async fn config_stored_schema<T: SettingStore + RuntimeStore>(
    db: &T,
    cipher: &SecretCipher,
    kind: SettingKind,
    scope: &str,
    name: &str,
) -> Result<Option<Value>, String> {
    if kind != SettingKind::Config {
        return Ok(None);
    }
    let record = db
        .fetch_setting(kind, scope.to_string(), name.to_string())
        .await
        .map_err(|err| err.to_string())?;
    Ok(record
        .and_then(|record| cipher.try_decrypt(&record.value))
        .and_then(|bytes| decode_config_schema(&bytes)))
}

pub async fn delete_credential<T: DefinitionStore + SettingStore + RuntimeStore>(
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

    let target = match db
        .fetch_setting(query.kind, scope.clone(), name.clone())
        .await
    {
        Ok(target) => target,
        Err(err) => return api_error(err.to_string()),
    };
    if let Some(target) = target {
        let workflows = match db.fetch_workflows().await {
            Ok(workflows) => workflows,
            Err(err) => return api_error(err.to_string()),
        };
        let inbound = workflows
            .into_iter()
            .filter(|workflow| {
                workflow
                    .definition
                    .metadata
                    .pointer("/artifact_refs/settings")
                    .and_then(Value::as_array)
                    .is_some_and(|bindings| {
                        bindings.iter().any(|binding| {
                            serde_json::from_value::<SettingBinding>(binding.clone().into())
                                .is_ok_and(|binding| binding.reference.id == target.id)
                        })
                    })
            })
            .map(|workflow| workflow.artifact_path().qualified())
            .collect::<Vec<_>>();
        if !inbound.is_empty() {
            return bad_request(format!(
                "setting {} is referenced by {}",
                target.id,
                inbound.join(", ")
            ));
        }
    }

    match db.delete_setting(query.kind, scope, name).await {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: true,
                message: "Credential deleted".into(),
            })),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn move_credential<T: DefinitionStore + SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(setting_id): Path<uuid::Uuid>,
    Json(request): Json<SettingMoveRequest>,
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
    let existing = match db.fetch_setting_by_id(setting_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return not_found("setting not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if existing.kind != request.kind {
        return bad_request("a setting move cannot change its kind");
    }
    match db
        .move_setting(setting_id, existing.kind, request.scope, request.name)
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
pub fn routes<T: DefinitionStore + SettingStore + RuntimeStore>(
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
