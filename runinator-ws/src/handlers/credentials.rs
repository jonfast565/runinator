use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;
use runinator_models::capabilities::Capability;
use runinator_models::value::Value;
use runinator_models::{
    bundles::{SecretBundle, SecretBundleEntry},
    settings::SettingKind,
    web::TaskResponse,
};
use runinator_utilities::secret_cipher::SecretCipher;

use crate::models::{ApiResponse, CredentialPutRequest, CredentialQuery};
use crate::responses::{api_error, bad_request, not_found};
use crate::settings::{decode_config_schema, decode_config_value, validate_and_encode};

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

pub(crate) async fn get_credential<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<CredentialQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_capability(&ctx, Capability::SecretsRead) {
        return reply;
    }
    let cipher = settings_cipher();
    if query.scope.is_none() && query.name.is_none() {
        return match db.list_settings().await {
            Ok(entries) => (
                StatusCode::OK,
                Json(ApiResponse::JsonList(
                    entries
                        .into_iter()
                        .map(|entry| {
                            runinator_models::json!({
                                "scope": entry.scope,
                                "name": entry.name,
                                "kind": entry.kind.as_str(),
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
            let value = match query.kind {
                SettingKind::Config => decode_config_value(&bytes),
                SettingKind::Secret => Value::String(String::from_utf8_lossy(&bytes).into_owned()),
            };
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(runinator_models::json!({
                    "scope": scope,
                    "name": name,
                    "kind": query.kind.as_str(),
                    "value": value.clone(),
                    // back-compat alias for existing secret consumers (e.g. the worker).
                    "secret": value,
                }))),
            )
        }
        Ok(None) => not_found("credential not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn put_credential<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<CredentialPutRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_capability(&ctx, Capability::SecretsWrite) {
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
    let bytes = match validate_and_encode(
        request.kind,
        &request.scope,
        &request.name,
        &request.value,
        request.schema.as_ref(),
        stored_schema.as_ref(),
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
                "stored": true
            }))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// re-encrypt every stored setting with the current primary key. used to complete a credential-key
/// rotation: run it while the old key is still configured as a secondary, then the old key can be
/// retired. idempotent — values already tagged with the primary key are left untouched.
pub(crate) async fn reencrypt_settings<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_capability(&ctx, Capability::SecretsWrite) {
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

pub(crate) async fn import_secret_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(bundle): Json<SecretBundle>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_capability(&ctx, Capability::SecretsWrite) {
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
pub(crate) struct SecretImportError {
    bad_request: bool,
    message: String,
}

impl SecretImportError {
    pub(crate) fn into_response(self) -> (StatusCode, Json<ApiResponse>) {
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
pub(crate) async fn import_secret_entries<T: DatabaseImpl>(
    db: &T,
    bundle: &SecretBundle,
) -> Result<Vec<SecretBundleEntry>, SecretImportError> {
    import_secret_entries_with(db, bundle, false).await
}

// `overwrite` makes an explicit re-apply authoritative: an existing setting is replaced even when
// the incoming entry is not strictly newer, bypassing the reconciliation timestamp gate.
pub(crate) async fn import_secret_entries_with<T: DatabaseImpl>(
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
        let bytes = validate_and_encode(
            secret.kind,
            &secret.scope,
            &secret.name,
            &secret.value,
            secret.schema.as_ref(),
            stored_schema.as_ref(),
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
async fn config_stored_schema<T: DatabaseImpl>(
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
    }
}

pub(crate) async fn delete_credential<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<CredentialQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_capability(&ctx, Capability::SecretsWrite) {
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
