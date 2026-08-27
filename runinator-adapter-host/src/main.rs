use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use runinator_adapter_contract::{
    ADAPTER_ABI_VERSION, AdapterMetadataEnvelope, AdapterRequest, AdapterResponse, FileOperationFn,
    HANDLE_SYMBOL, MARKER_SYMBOL, METADATA_SYMBOL, MarkerFn, NAME_SYMBOL, NameFn,
};
use runinator_models::{
    orchestration::{AdapterConfigurationField, AdapterKindMetadata, NormalizedAdapterEvent},
    types::RuninatorType,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{process::Command, sync::RwLock, time::timeout};

const DEFAULT_BODY_LIMIT: usize = 1024 * 1024;
const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;
const DEFAULT_EVENT_LIMIT: usize = 16;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
struct CatalogEntry {
    metadata: AdapterKindMetadata,
    origin: String,
    healthy: bool,
    error: Option<String>,
}

#[derive(Clone)]
struct HostState {
    token: Arc<String>,
    paths: Arc<Vec<PathBuf>>,
    catalog: Arc<RwLock<BTreeMap<String, CatalogEntry>>>,
}

#[derive(Debug, Deserialize)]
struct InvokeRequest {
    kind: String,
    request: AdapterRequest,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) == Some("--child-metadata") {
        return child_metadata(
            Path::new(required_arg(&args, 2)?),
            Path::new(required_arg(&args, 3)?),
        );
    }
    if args.get(1).map(String::as_str) == Some("--child-handle") {
        return child_handle(
            Path::new(required_arg(&args, 2)?),
            Path::new(required_arg(&args, 3)?),
            Path::new(required_arg(&args, 4)?),
        );
    }

    let token = std::env::var("RUNINATOR_ADAPTER_HOST_TOKEN")
        .map_err(|_| "RUNINATOR_ADAPTER_HOST_TOKEN is required")?;
    let paths = std::env::var_os("RUNINATOR_ADAPTER_PLUGIN_PATHS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    let state = HostState {
        token: Arc::new(token),
        paths: Arc::new(paths),
        catalog: Arc::new(RwLock::new(BTreeMap::new())),
    };
    reload_catalog(&state).await;
    let port = std::env::var("RUNINATOR_ADAPTER_HOST_PORT")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(8790);
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    let router = Router::new()
        .route("/health", get(health))
        .route("/kinds", get(kinds))
        .route("/reload", post(reload))
        .route("/verify-normalize", post(invoke))
        .with_state(state);
    axum::serve(listener, router).await?;
    Ok(())
}

fn required_arg(args: &[String], index: usize) -> Result<&str, Box<dyn std::error::Error>> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing argument {index}").into())
}

async fn health(State(state): State<HostState>, headers: HeaderMap) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let catalog = state.catalog.read().await;
    (
        StatusCode::OK,
        Json(json!({
            "healthy": catalog.values().all(|entry| entry.healthy),
            "kinds": catalog.len(),
        })),
    )
}

async fn kinds(State(state): State<HostState>, headers: HeaderMap) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let entries = state
        .catalog
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    (
        StatusCode::OK,
        Json(serde_json::to_value(entries).unwrap_or_default()),
    )
}

async fn reload(State(state): State<HostState>, headers: HeaderMap) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    reload_catalog(&state).await;
    (StatusCode::OK, Json(json!({ "reloaded": true })))
}

async fn invoke(
    State(state): State<HostState>,
    headers: HeaderMap,
    Json(request): Json<InvokeRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    if request.request.body_base64.len() > DEFAULT_BODY_LIMIT.saturating_mul(2) {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": "request body exceeds limit" })),
        );
    }
    let entry = state.catalog.read().await.get(&request.kind).cloned();
    let Some(entry) = entry else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "adapter kind not found" })),
        );
    };
    let response = if entry.origin == "builtin" {
        builtin_handle(&request.kind, request.request)
    } else {
        invoke_dynamic(Path::new(&entry.origin), &request.request)
            .await
            .unwrap_or_else(|error| AdapterResponse::rejected(error))
    };
    if response.events.len() > DEFAULT_EVENT_LIMIT {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "adapter emitted too many events" })),
        );
    }
    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap_or_default()),
    )
}

fn authorized(state: &HostState, headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| {
            constant_time_eq::constant_time_eq(value.as_bytes(), state.token.as_bytes())
        })
}

fn unauthorized() -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "invalid adapter-host token" })),
    )
}

async fn reload_catalog(state: &HostState) {
    let mut catalog = builtin_catalog();
    for directory in state.paths.iter() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_library(&path) {
                continue;
            }
            match dynamic_metadata(&path).await {
                Ok(metadata) => {
                    catalog.insert(
                        metadata.kind.clone(),
                        CatalogEntry {
                            metadata,
                            origin: path.to_string_lossy().into_owned(),
                            healthy: true,
                            error: None,
                        },
                    );
                }
                Err(error) => {
                    let key = format!(
                        "invalid:{}",
                        path.file_name()
                            .and_then(|v| v.to_str())
                            .unwrap_or("plugin")
                    );
                    catalog.insert(
                        key,
                        CatalogEntry {
                            metadata: placeholder_metadata(&path),
                            origin: path.to_string_lossy().into_owned(),
                            healthy: false,
                            error: Some(error),
                        },
                    );
                }
            }
        }
    }
    *state.catalog.write().await = catalog;
}

fn is_library(path: &Path) -> bool {
    let expected = if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    path.extension().and_then(|value| value.to_str()) == Some(expected)
}

async fn dynamic_metadata(path: &Path) -> Result<AdapterKindMetadata, String> {
    let temp = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let status = timeout(
        DEFAULT_TIMEOUT,
        Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
            .arg("--child-metadata")
            .arg(path)
            .arg(temp.path())
            .status(),
    )
    .await
    .map_err(|_| "adapter metadata timed out".to_string())?
    .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("metadata child exited with {status}"));
    }
    let bytes = std::fs::read(temp.path()).map_err(|error| error.to_string())?;
    if bytes.len() > DEFAULT_OUTPUT_LIMIT {
        return Err("metadata output exceeds limit".into());
    }
    let envelope: AdapterMetadataEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if envelope.abi_version != ADAPTER_ABI_VERSION {
        return Err("adapter ABI version mismatch".into());
    }
    Ok(envelope.metadata)
}

async fn invoke_dynamic(path: &Path, request: &AdapterRequest) -> Result<AdapterResponse, String> {
    let request_file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let response_file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let request_bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    if request_bytes.len() > DEFAULT_BODY_LIMIT {
        return Err("adapter request exceeds limit".into());
    }
    std::fs::write(request_file.path(), request_bytes).map_err(|error| error.to_string())?;
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--child-handle")
        .arg(path)
        .arg(request_file.path())
        .arg(response_file.path())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let status = timeout(DEFAULT_TIMEOUT, child.wait())
        .await
        .map_err(|_| "adapter invocation timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("adapter child exited with {status}"));
    }
    let bytes = std::fs::read(response_file.path()).map_err(|error| error.to_string())?;
    if bytes.len() > DEFAULT_OUTPUT_LIMIT {
        return Err("adapter output exceeds limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn child_metadata(
    library_path: &Path,
    response_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: all dynamically loaded code is contained in this disposable child process.
    unsafe {
        let library = libloading::Library::new(library_path)?;
        let marker = library.get::<MarkerFn>(MARKER_SYMBOL)?;
        if marker() != ADAPTER_ABI_VERSION {
            return Err("adapter ABI version mismatch".into());
        }
        let name = library.get::<NameFn>(NAME_SYMBOL)?;
        let _ = CStr::from_ptr(name()).to_str()?;
        let operation = library.get::<FileOperationFn>(METADATA_SYMBOL)?;
        invoke_file_operation(*operation, None, response_path)?;
    }
    Ok(())
}

fn child_handle(
    library_path: &Path,
    request_path: &Path,
    response_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: all dynamically loaded code is contained in this disposable child process.
    unsafe {
        let library = libloading::Library::new(library_path)?;
        let marker = library.get::<MarkerFn>(MARKER_SYMBOL)?;
        if marker() != ADAPTER_ABI_VERSION {
            return Err("adapter ABI version mismatch".into());
        }
        let operation = library.get::<FileOperationFn>(HANDLE_SYMBOL)?;
        invoke_file_operation(*operation, Some(request_path), response_path)?;
    }
    Ok(())
}

unsafe fn invoke_file_operation(
    operation: FileOperationFn,
    request: Option<&Path>,
    response: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = CString::new(
        request
            .map(|path| path.to_string_lossy())
            .unwrap_or_default()
            .as_bytes(),
    )?;
    let response = CString::new(response.to_string_lossy().as_bytes())?;
    // SAFETY: the loaded function was resolved by the versioned symbol contract and receives valid C strings.
    if unsafe { operation(request.as_ptr(), response.as_ptr()) } != 0 {
        return Err("adapter operation failed".into());
    }
    Ok(())
}

fn builtin_catalog() -> BTreeMap<String, CatalogEntry> {
    [generic_metadata(), jira_metadata(), github_metadata()]
        .into_iter()
        .map(|metadata| {
            (
                metadata.kind.clone(),
                CatalogEntry {
                    metadata,
                    origin: "builtin".into(),
                    healthy: true,
                    error: None,
                },
            )
        })
        .collect()
}

fn placeholder_metadata(path: &Path) -> AdapterKindMetadata {
    AdapterKindMetadata {
        kind: format!("invalid:{}", path.display()),
        version: "unknown".into(),
        display_name: "Invalid adapter".into(),
        description: None,
        fields: vec![],
        event_names: vec![],
        canonical_pointers: vec![],
        capabilities: vec![],
    }
}

fn field(
    name: &str,
    value_type: RuninatorType,
    required: bool,
    secret: bool,
) -> AdapterConfigurationField {
    AdapterConfigurationField {
        name: name.into(),
        value_type,
        required,
        secret,
        description: None,
        default: Value::Null.into(),
    }
}

fn generic_metadata() -> AdapterKindMetadata {
    AdapterKindMetadata {
        kind: "generic_webhook".into(),
        version: "1".into(),
        display_name: "Generic webhook".into(),
        description: Some("HMAC-SHA256 or bearer-authenticated JSON webhook".into()),
        fields: vec![
            field("authentication", RuninatorType::String, true, false),
            field("secret", RuninatorType::String, true, true),
            field("delivery_id_pointer", RuninatorType::String, true, false),
            field("scope_pointer", RuninatorType::String, true, false),
            field("correlation_pointer", RuninatorType::String, true, false),
            field("event_pointer", RuninatorType::String, true, false),
            field("provenance_pointer", RuninatorType::String, false, false),
        ],
        event_names: vec![],
        canonical_pointers: vec![
            "/delivery_id".into(),
            "/scope".into(),
            "/correlation_key".into(),
            "/event_type".into(),
        ],
        capabilities: vec!["hmac_sha256".into(), "bearer".into()],
    }
}

fn jira_metadata() -> AdapterKindMetadata {
    AdapterKindMetadata {
        kind: "jira".into(),
        version: "1".into(),
        display_name: "Jira".into(),
        description: Some("Canonical Jira issue, change, and comment events".into()),
        fields: vec![field("secret", RuninatorType::String, true, true)],
        event_names: vec!["issue_updated".into(), "comment_created".into()],
        canonical_pointers: vec![
            "/issue/id".into(),
            "/issue/key".into(),
            "/changes".into(),
            "/provenance".into(),
        ],
        capabilities: vec!["bearer".into()],
    }
}

fn github_metadata() -> AdapterKindMetadata {
    AdapterKindMetadata {
        kind: "github".into(),
        version: "1".into(),
        display_name: "GitHub".into(),
        description: Some("Canonical repository, pull request, check, and workflow events".into()),
        fields: vec![field("secret", RuninatorType::String, true, true)],
        event_names: vec![
            "pull_request".into(),
            "check_run".into(),
            "workflow_run".into(),
        ],
        canonical_pointers: vec![
            "/repository/id".into(),
            "/pull_request/id".into(),
            "/subject_revision".into(),
            "/provenance".into(),
        ],
        capabilities: vec!["hmac_sha256".into()],
    }
}

fn builtin_handle(kind: &str, request: AdapterRequest) -> AdapterResponse {
    match kind {
        "generic_webhook" => handle_generic(request),
        "github" => handle_github(request),
        "jira" => handle_jira(request),
        _ => AdapterResponse::rejected("unknown built-in adapter"),
    }
}

fn decode_body(request: &AdapterRequest) -> Result<(Vec<u8>, Value), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&request.body_base64)
        .map_err(|e| e.to_string())?;
    if bytes.len() > DEFAULT_BODY_LIMIT {
        return Err("request body exceeds limit".into());
    }
    let json = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok((bytes, json))
}

fn configured_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing configuration '{key}'"))
}

fn pointer_string(value: &Value, pointer: &str) -> Result<String, String> {
    let value = value
        .pointer(pointer)
        .ok_or_else(|| format!("pointer '{pointer}' did not resolve"))?;
    Ok(value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string()))
}

fn verify_hmac(secret: &str, bytes: &[u8], supplied: &str) -> bool {
    use hmac::{Hmac, Mac};
    let supplied = supplied.strip_prefix("sha256=").unwrap_or(supplied);
    let Ok(expected) = hex_decode(supplied) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(bytes);
    mac.verify_slice(&expected).is_ok()
}

fn hex_decode(value: &str) -> Result<Vec<u8>, ()> {
    if !value.len().is_multiple_of(2) {
        return Err(());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(|_| ()))
        .collect()
}

fn handle_generic(request: AdapterRequest) -> AdapterResponse {
    let Ok((bytes, payload)) = decode_body(&request) else {
        return AdapterResponse::rejected("invalid JSON body");
    };
    let mode = configured_string(&request.configuration, "authentication").unwrap_or("hmac_sha256");
    let secret = configured_string(&request.secrets, "secret")
        .or_else(|_| configured_string(&request.configuration, "secret"));
    let Ok(secret) = secret else {
        return AdapterResponse::rejected("missing verification secret");
    };
    let verified = match mode {
        "hmac_sha256" => request
            .headers
            .get("x-runinator-signature")
            .is_some_and(|value| verify_hmac(secret, &bytes, value)),
        "bearer" => request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|value| {
                constant_time_eq::constant_time_eq(value.as_bytes(), secret.as_bytes())
            }),
        _ => false,
    };
    if !verified {
        return AdapterResponse::rejected("webhook verification failed");
    }
    let result = (|| {
        let delivery_id = pointer_string(
            &payload,
            configured_string(&request.configuration, "delivery_id_pointer")?,
        )?;
        let scope = pointer_string(
            &payload,
            configured_string(&request.configuration, "scope_pointer")?,
        )?;
        let correlation_key = pointer_string(
            &payload,
            configured_string(&request.configuration, "correlation_pointer")?,
        )?;
        let event_type = pointer_string(
            &payload,
            configured_string(&request.configuration, "event_pointer")?,
        )?;
        let provenance = request
            .configuration
            .get("provenance_pointer")
            .and_then(Value::as_str)
            .and_then(|pointer| payload.pointer(pointer))
            .cloned()
            .unwrap_or(Value::Null);
        Ok::<_, String>(NormalizedAdapterEvent {
            source: "generic_webhook".into(),
            delivery_id,
            event_type,
            scope,
            correlation_key,
            subject_revision: request
                .configuration
                .get("subject_revision_pointer")
                .and_then(Value::as_str)
                .and_then(|pointer| pointer_string(&payload, pointer).ok()),
            occurred_at: None,
            payload: payload.into(),
            provenance: provenance.into(),
        })
    })();
    match result {
        Ok(event) => AdapterResponse {
            verified: true,
            events: vec![event],
            errors: vec![],
        },
        Err(error) => AdapterResponse::rejected(error),
    }
}

fn handle_github(request: AdapterRequest) -> AdapterResponse {
    let Ok((bytes, payload)) = decode_body(&request) else {
        return AdapterResponse::rejected("invalid GitHub JSON body");
    };
    let secret = configured_string(&request.secrets, "secret")
        .or_else(|_| configured_string(&request.configuration, "secret"));
    let signature = request.headers.get("x-hub-signature-256");
    if secret
        .ok()
        .zip(signature)
        .is_none_or(|(secret, signature)| !verify_hmac(secret, &bytes, signature))
    {
        return AdapterResponse::rejected("GitHub signature verification failed");
    }
    let delivery_id = request
        .headers
        .get("x-github-delivery")
        .cloned()
        .unwrap_or_default();
    let event_type = request
        .headers
        .get("x-github-event")
        .cloned()
        .unwrap_or_default();
    if delivery_id.is_empty() || event_type.is_empty() {
        return AdapterResponse::rejected("missing GitHub delivery headers");
    }
    let repository = payload
        .pointer("/repository/id")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let correlation_key = payload
        .pointer("/pull_request/id")
        .or_else(|| payload.pointer("/check_run/id"))
        .or_else(|| payload.pointer("/workflow_run/id"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| repository.clone());
    let subject_revision = payload
        .pointer("/pull_request/head/sha")
        .or_else(|| payload.pointer("/check_run/head_sha"))
        .or_else(|| payload.pointer("/workflow_run/head_sha"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let provenance = extract_runinator_operation_key(&payload)
        .map(|operation_key| json!({ "operation_key": operation_key }))
        .unwrap_or(Value::Null);
    AdapterResponse {
        verified: true,
        events: vec![NormalizedAdapterEvent {
            source: "github".into(),
            delivery_id,
            event_type,
            scope: repository,
            correlation_key,
            subject_revision,
            occurred_at: None,
            payload: payload.into(),
            provenance: provenance.into(),
        }],
        errors: vec![],
    }
}

fn handle_jira(request: AdapterRequest) -> AdapterResponse {
    let Ok((_, payload)) = decode_body(&request) else {
        return AdapterResponse::rejected("invalid Jira JSON body");
    };
    let secret = configured_string(&request.secrets, "secret")
        .or_else(|_| configured_string(&request.configuration, "secret"));
    let verified = secret.ok().is_some_and(|secret| {
        request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|value| {
                constant_time_eq::constant_time_eq(value.as_bytes(), secret.as_bytes())
            })
    });
    if !verified {
        return AdapterResponse::rejected("Jira authentication failed");
    }
    let delivery_id = request
        .headers
        .get("x-atlassian-webhook-identifier")
        .cloned()
        .or_else(|| payload.get("timestamp").map(Value::to_string))
        .unwrap_or_default();
    let event_type = payload
        .get("webhookEvent")
        .and_then(Value::as_str)
        .unwrap_or("jira_event")
        .to_owned();
    let issue_id = payload
        .pointer("/issue/id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let project_id = payload
        .pointer("/issue/fields/project/id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if delivery_id.is_empty() || issue_id.is_empty() {
        return AdapterResponse::rejected("Jira event lacks stable delivery or issue identity");
    }
    let provenance = extract_runinator_operation_key(&payload)
        .map(|operation_key| json!({ "operation_key": operation_key }))
        .unwrap_or(Value::Null);
    AdapterResponse {
        verified: true,
        events: vec![NormalizedAdapterEvent {
            source: "jira".into(),
            delivery_id,
            event_type,
            scope: project_id,
            correlation_key: issue_id,
            subject_revision: None,
            occurred_at: None,
            payload: payload.into(),
            provenance: provenance.into(),
        }],
        errors: vec![],
    }
}

fn extract_runinator_operation_key(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => {
            let marker = "runinator-operation:";
            let start = value.find(marker)? + marker.len();
            let key = value[start..]
                .split(|character: char| {
                    character.is_whitespace() || matches!(character, ']' | '<' | '>' | '"')
                })
                .next()
                .unwrap_or_default()
                .trim_matches('-');
            (!key.is_empty()).then(|| key.to_string())
        }
        Value::Array(values) => values.iter().find_map(extract_runinator_operation_key),
        Value::Object(values) => values.values().find_map(extract_runinator_operation_key),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use hmac::{Hmac, Mac};

    fn signature(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!(
            "sha256={}",
            mac.finalize()
                .into_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }

    #[test]
    fn generic_webhook_verifies_and_normalizes_configured_identity() {
        let body = br#"{"delivery":"d-1","tenant":"acme","object":{"id":"42"},"event":"changed"}"#;
        let response = handle_generic(AdapterRequest {
            method: "POST".into(),
            headers: BTreeMap::from([(
                "x-runinator-signature".into(),
                signature("test-secret", body),
            )]),
            body_base64: base64::engine::general_purpose::STANDARD.encode(body),
            configuration: json!({
                "authentication": "hmac_sha256",
                "delivery_id_pointer": "/delivery",
                "scope_pointer": "/tenant",
                "correlation_pointer": "/object/id",
                "event_pointer": "/event"
            }),
            secrets: json!({ "secret": "test-secret" }),
        });
        assert!(response.verified);
        assert_eq!(response.events[0].delivery_id, "d-1");
        assert_eq!(response.events[0].scope, "acme");
        assert_eq!(response.events[0].correlation_key, "42");
    }

    #[test]
    fn github_rejects_a_signature_for_different_bytes() {
        let body = br#"{"repository":{"id":1}}"#;
        let response = handle_github(AdapterRequest {
            method: "POST".into(),
            headers: BTreeMap::from([
                (
                    "x-hub-signature-256".into(),
                    signature("secret", b"different"),
                ),
                ("x-github-delivery".into(), "delivery".into()),
                ("x-github-event".into(), "push".into()),
            ]),
            body_base64: base64::engine::general_purpose::STANDARD.encode(body),
            configuration: Value::Null,
            secrets: json!({ "secret": "secret" }),
        });
        assert!(!response.verified);
        assert!(response.events.is_empty());
    }

    #[test]
    fn provider_markers_are_normalized_as_operation_provenance() {
        for marker in [
            "<!-- runinator-operation:operation-42 -->",
            "[runinator-operation:operation-42]",
        ] {
            assert_eq!(
                extract_runinator_operation_key(&json!({ "body": marker })).as_deref(),
                Some("operation-42")
            );
        }
    }
}
