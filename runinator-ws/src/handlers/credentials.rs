use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use axum::{Json, extract::Query, http::StatusCode};
use runinator_models::value::{Map, Value};
use runinator_models::{
    bundles::{SecretBundle, SecretBundleEntry},
    settings::SettingKind,
    web::TaskResponse,
};
use runinator_utilities::credential_store::{
    CredentialStore, LocalEncryptedCredentialStore, default_app_credential_store_path,
};

use crate::models::{ApiResponse, CredentialPutRequest, CredentialQuery};
use crate::responses::{api_error, bad_request, not_found};
use crate::settings::{decode_config_value, validate_and_encode};

// resolve the credential store path from the environment, falling back to the app-data default.
fn credential_store_path() -> PathBuf {
    std::env::var("RUNINATOR_CREDENTIAL_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            default_app_credential_store_path()
                .unwrap_or_else(|_| PathBuf::from("credentials.enc.json"))
        })
}

pub(crate) fn credential_store() -> LocalEncryptedCredentialStore {
    let key = std::env::var("RUNINATOR_CREDENTIAL_KEY")
        .unwrap_or_else(|_| "runinator-local-development-key".into());
    LocalEncryptedCredentialStore::new(credential_store_path(), key)
}

// caches the built config tree keyed on the store file's modification time, so the reducer
// (which calls config_tree on every context build) rereads the file only when it changes.
static CONFIG_CACHE: Mutex<Option<(Option<SystemTime>, Value)>> = Mutex::new(None);

// the store file's modification time, or None when it does not exist yet.
fn config_store_mtime() -> Option<SystemTime> {
    std::fs::metadata(credential_store_path())
        .and_then(|meta| meta.modified())
        .ok()
}

/// the config reference tree `{ <scope>: { <name>: <value> } }`, cached and invalidated by the
/// store file's mtime. secrets are never included — they resolve late at the worker.
pub(crate) fn config_tree() -> Value {
    let mtime = config_store_mtime();
    if let Ok(cache) = CONFIG_CACHE.lock() {
        if let Some((cached_mtime, value)) = cache.as_ref() {
            if *cached_mtime == mtime {
                return value.clone();
            }
        }
    }
    let tree = build_config_tree();
    if let Ok(mut cache) = CONFIG_CACHE.lock() {
        *cache = Some((mtime, tree.clone()));
    }
    tree
}

fn build_config_tree() -> Value {
    let store = credential_store();
    let Ok(entries) = store.list() else {
        return Value::Object(Map::new());
    };
    let mut root = Map::new();
    for entry in entries {
        if entry.kind != SettingKind::Config {
            continue;
        }
        let Ok(Some(bytes)) = store.get(SettingKind::Config, &entry.scope, &entry.name) else {
            continue;
        };
        let value = decode_config_value(&bytes);
        let scope = root
            .entry(entry.scope)
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(scope) = scope.as_object_mut() {
            scope.insert(entry.name, value);
        }
    }
    Value::Object(root)
}

pub(crate) async fn get_credential(
    Query(query): Query<CredentialQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let store = credential_store();
    if query.scope.is_none() && query.name.is_none() {
        return match store.list() {
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

    match store.get(query.kind, &scope, &name) {
        // config is non-sensitive: return the parsed json value. secrets return the raw string.
        Ok(Some(bytes)) => {
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

pub(crate) async fn put_credential(
    Json(request): Json<CredentialPutRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let store = credential_store();
    let bytes = match validate_and_encode(
        &store,
        request.kind,
        &request.scope,
        &request.name,
        &request.value,
        request.schema.as_ref(),
    ) {
        Ok(bytes) => bytes,
        Err(message) => return bad_request(message),
    };
    match store.put(request.kind, &request.scope, &request.name, &bytes) {
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

pub(crate) async fn import_secret_bundle(
    Json(bundle): Json<SecretBundle>,
) -> (StatusCode, Json<ApiResponse>) {
    let store = credential_store();
    let mut imported = Vec::with_capacity(bundle.secrets.len());
    for secret in &bundle.secrets {
        let incoming_ts = secret.updated_at.map(|updated_at| updated_at.timestamp());
        // overwrite an existing entry only when the incoming entry is strictly newer.
        match store.entry_updated_at(secret.kind, &secret.scope, &secret.name) {
            Ok(Some(stored_ts)) => {
                let is_newer = incoming_ts.map(|ts| ts > stored_ts).unwrap_or(false);
                if !is_newer {
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
            Ok(None) => {}
            Err(err) => return api_error(err.to_string()),
        }
        // validate against the declared (or previously stored) schema before persisting.
        let bytes = match validate_and_encode(
            &store,
            secret.kind,
            &secret.scope,
            &secret.name,
            &secret.value,
            secret.schema.as_ref(),
        ) {
            Ok(bytes) => bytes,
            Err(message) => return bad_request(message),
        };
        // persist the incoming modification time so later imports reconcile against it.
        let result = match incoming_ts {
            Some(ts) => store.put_at(secret.kind, &secret.scope, &secret.name, &bytes, ts),
            None => store.put(secret.kind, &secret.scope, &secret.name, &bytes),
        };
        if let Err(err) = result {
            return api_error(err.to_string());
        }
        imported.push(redacted_entry(secret));
    }

    (
        StatusCode::OK,
        Json(ApiResponse::SecretBundle(SecretBundle {
            secrets: imported,
        })),
    )
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

pub(crate) async fn delete_credential(
    Query(query): Query<CredentialQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let (Some(scope), Some(name)) = (query.scope, query.name) else {
        return bad_request("credential deletion requires both scope and name");
    };

    match credential_store().delete(query.kind, &scope, &name) {
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
