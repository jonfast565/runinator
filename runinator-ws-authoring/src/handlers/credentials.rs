use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_models::auth::AuthContext;
use runinator_models::value::Value;
use runinator_models::{
    bundles::{SecretBundle, SecretBundleEntry},
    settings::SettingKind,
    web::TaskResponse,
};
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_store::{RuntimeStore, roles::SettingStore};

use crate::settings::{
    decode_config_schema, decode_config_value, decode_secret, validate_and_encode_with_expiry,
};
use runinator_ws_core::models::{ApiResponse, CredentialPutRequest, CredentialQuery};
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
                                .and_then(|bytes| decode_secret(&bytes).expires_at);
                            runinator_models::json!({
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
                    let secret = decode_secret(&bytes);
                    (Value::String(secret.value), secret.expires_at)
                }
            };
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(runinator_models::json!({
                    "scope": scope,
                    "name": name,
                    "kind": query.kind.as_str(),
                    "value": value.clone(),
                    "expires_at": expires_at,
                    // back-compat alias for existing secret consumers (e.g. the worker).
                    "secret": value,
                }))),
            )
        }
        Ok(None) => not_found("credential not found"),
        Err(err) => api_error(err.to_string()),
    }
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

/// re-encrypt every stored setting with the current primary key. used to complete a credential-key
/// rotation: run it while the old key is still configured as a secondary, then the old key can be
/// retired. idempotent — values already tagged with the primary key are left untouched.
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

pub async fn import_secret_bundle<T: SettingStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(bundle): Json<SecretBundle>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::SecretsWrite,
        ctx.selected_scope(),
    ) {
        return reply;
    }
    match import_secret_entries(db.as_ref(), &bundle).await {
        Ok(imported) => (
            StatusCode::OK,
            Json(ApiResponse::SecretBundle(SecretBundle {
                secrets: imported,
            })),
        ),
        Err(error) => error.into_response(),
    }
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

/// import every entry in a secret bundle into the settings store, reconciling by modification time,
/// and return the redacted echo. shared by the json `/credentials/import` endpoint and the compiled
/// pack import at `/packs/import`.
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
    let cipher = settings_cipher();
    let mut imported = Vec::with_capacity(bundle.secrets.len());
    for secret in &bundle.secrets {
        let incoming_ts = secret.updated_at.map(|updated_at| updated_at.timestamp());
        // load the stored record once: it gates reconciliation and pins the config schema.
        let stored = db
            .fetch_setting(secret.kind, secret.scope.clone(), secret.name.clone())
            .await
            .map_err(|err| SecretImportError {
                bad_request: false,
                message: err.to_string(),
            })?;
        // overwrite an existing entry only on an explicit overwrite or when the incoming entry is
        // strictly newer.
        if let Some(stored) = &stored {
            let is_newer = incoming_ts
                .map(|ts| ts > stored.updated_at)
                .unwrap_or(false);
            if !overwrite && !is_newer {
                log::info!(
                    "Skipping import of {} {}/{}: stored copy is up to date",
                    secret.kind.as_str(),
                    secret.scope,
                    secret.name
                );
                imported.push(redacted_entry(secret));
                continue;
            }
        }
        // validate against the declared (or previously stored) schema before persisting.
        let stored_schema = stored
            .as_ref()
            .and_then(|record| decode_config_schema(&cipher.decrypt(&record.value)));
        let bytes = validate_and_encode_with_expiry(
            secret.kind,
            &secret.scope,
            &secret.name,
            &secret.value,
            secret.schema.as_ref(),
            stored_schema.as_ref(),
            secret.expires_at,
        )
        .map_err(|message| SecretImportError {
            bad_request: true,
            message,
        })?;
        // persist the incoming modification time so later imports reconcile against it.
        let updated_at = incoming_ts.unwrap_or_else(now_unix);
        let ciphertext = cipher.encrypt(&bytes);
        db.upsert_setting(
            secret.kind,
            secret.scope.clone(),
            secret.name.clone(),
            ciphertext,
            updated_at,
        )
        .await
        .map_err(|err| SecretImportError {
            bad_request: false,
            message: err.to_string(),
        })?;
        imported.push(redacted_entry(secret));
    }
    Ok(imported)
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

// echo an imported entry without its value, preserving kind and modification time.
fn redacted_entry(secret: &SecretBundleEntry) -> SecretBundleEntry {
    SecretBundleEntry {
        scope: secret.scope.clone(),
        name: secret.name.clone(),
        value: Value::Null,
        schema: None,
        kind: secret.kind,
        updated_at: secret.updated_at,
        expires_at: secret.expires_at,
    }
}

pub async fn delete_credential<T: SettingStore + RuntimeStore>(
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

/// the `credentials` endpoints.
pub fn routes<T: SettingStore + RuntimeStore>(pool: std::sync::Arc<T>) -> axum::Router {
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
            "/credentials/import",
            post(import_secret_bundle::<T>).layer(Extension(pool.clone())),
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
        "/credentials/import",
        "Credentials",
        "Import a secret bundle",
        "Imports secret/config entries from a compiled pack secret bundle.",
        false,
        json_body("Secret bundle to import.", Example::SecretBundle),
        &[],
        200,
        "secret bundle imported",
        Example::SecretBundle,
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
