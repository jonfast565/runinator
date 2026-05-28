use axum::{Json, extract::Query, http::StatusCode};
use runinator_models::{
    bundles::{SecretBundle, SecretBundleEntry},
    web::TaskResponse,
};
use runinator_utilities::credential_store::{
    CredentialStore, LocalEncryptedCredentialStore, default_app_credential_store_path,
};

use crate::models::{ApiResponse, CredentialPutRequest, CredentialQuery};
use crate::responses::{api_error, bad_request, not_found};

pub(crate) fn credential_store() -> LocalEncryptedCredentialStore {
    let path = std::env::var("RUNINATOR_CREDENTIAL_STORE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            default_app_credential_store_path()
                .unwrap_or_else(|_| std::path::PathBuf::from("credentials.enc.json"))
        });
    let key = std::env::var("RUNINATOR_CREDENTIAL_KEY")
        .unwrap_or_else(|_| "runinator-local-development-key".into());
    LocalEncryptedCredentialStore::new(path, key)
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
                            serde_json::json!({
                                "scope": entry.scope,
                                "name": entry.name,
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

    match store.get(&scope, &name) {
        Ok(Some(secret)) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(serde_json::json!({
                "scope": scope,
                "name": name,
                "secret": String::from_utf8_lossy(&secret)
            }))),
        ),
        Ok(None) => not_found("credential not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn put_credential(
    Json(request): Json<CredentialPutRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match credential_store().put(&request.scope, &request.name, request.secret.as_bytes()) {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(serde_json::json!({
                "scope": request.scope,
                "name": request.name,
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
        // overwrite an existing secret only when the incoming entry is strictly newer.
        match store.entry_updated_at(&secret.scope, &secret.name) {
            Ok(Some(stored_ts)) => {
                let is_newer = incoming_ts.map(|ts| ts > stored_ts).unwrap_or(false);
                if !is_newer {
                    log::info!(
                        "Skipping import of secret {}/{}: stored copy is up to date",
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
        // persist the incoming modification time so later imports reconcile against it.
        let result = match incoming_ts {
            Some(ts) => store.put_at(&secret.scope, &secret.name, secret.secret.as_bytes(), ts),
            None => store.put(&secret.scope, &secret.name, secret.secret.as_bytes()),
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

// echo an imported entry without its secret value, preserving the modification time.
fn redacted_entry(secret: &SecretBundleEntry) -> SecretBundleEntry {
    SecretBundleEntry {
        scope: secret.scope.clone(),
        name: secret.name.clone(),
        secret: String::new(),
        updated_at: secret.updated_at,
    }
}

pub(crate) async fn delete_credential(
    Query(query): Query<CredentialQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let (Some(scope), Some(name)) = (query.scope, query.name) else {
        return bad_request("credential deletion requires both scope and name");
    };

    match credential_store().delete(&scope, &name) {
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
