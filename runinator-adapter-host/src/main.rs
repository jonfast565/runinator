mod builtins;
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use runinator_adapter_contract::{
    ADAPTER_ABI_VERSION, AdapterImmediateResponse, AdapterMetadataEnvelope, AdapterPollRequest,
    AdapterPollResponse, AdapterRequest, AdapterResponse, AdapterValidationRequest,
    AdapterValidationResponse, FileOperationFn, HANDLE_SYMBOL, MARKER_SYMBOL, METADATA_SYMBOL,
    MarkerFn, NAME_SYMBOL, NameFn, POLL_SYMBOL, VALIDATE_SYMBOL, StreamCheckpoints, call_symbol,
    cstr_to_rust_string, find_marker, invoke_file_operation, verify_bearer, verify_hmac_sha256,
};
use runinator_github::{AsyncGitHubClient, GitHubOperation};
use runinator_jira::{AsyncJiraClient, JiraCredentials, JiraOperation};
use runinator_models::{
    orchestration::{
        AdapterAuthenticationKind, AdapterConfigurationField, AdapterKindCatalogEntry,
        AdapterKindMetadata, NormalizedAdapterEvent,
    },
    types::{RuninatorField, RuninatorType},
};
use runinator_platform::{env, time};
use runinator_provider_support::polling::{PollLimit, PollPage, poll_pages_async};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{process::Command, sync::RwLock, time::timeout};

const DEFAULT_BODY_LIMIT: usize = 1024 * 1024;
const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;
const DEFAULT_EVENT_LIMIT: usize = 16;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Paging and fan-out budgets for the built-in pollers. These bound one poll's cost against the
/// provider's hourly quota: without a page cap a large backlog would walk indefinitely, and
/// without a commit budget a busy repository would spend the whole GitHub quota on check-run
/// lookups in a single pass.
const GITHUB_MAX_PAGES: usize = 10;
const GITHUB_CHECK_RUN_COMMIT_BUDGET: usize = 20;
const JIRA_MAX_PAGES: usize = 100;
/// Clock skew allowance between this host and Jira, and the furthest back a relative bound reaches.
const JIRA_SKEW_MARGIN_MINUTES: i64 = 5;
const JIRA_MAX_LOOKBACK_MINUTES: i64 = 90 * 24 * 60;

#[allow(dead_code)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if run_child_command(&args).await? {
        return Ok(());
    }

    let process = runinator_platform::startup::ProcessResources::start("Runinator Adapter Host")
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    let token = env::string("RUNINATOR_ADAPTER_HOST_TOKEN")
        .ok_or("RUNINATOR_ADAPTER_HOST_TOKEN is required")?;

    let paths = env::paths("RUNINATOR_ADAPTER_PLUGIN_PATHS");

    let port = env::parse_or("RUNINATOR_ADAPTER_HOST_PORT", 8790);
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    let shutdown = process.shutdown().clone();
    serve(
        listener,
        token,
        paths,
        async move { shutdown.cancelled().await },
    )
    .await
}

pub async fn run_child_command(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if args.get(1).map(String::as_str) == Some("--child-metadata") {
        child_metadata(
            Path::new(required_arg(args, 2)?),
            Path::new(required_arg(args, 3)?),
        )?;
        return Ok(true);
    }
    if args.get(1).map(String::as_str) == Some("--child-handle") {
        child_handle(
            Path::new(required_arg(args, 2)?),
            Path::new(required_arg(args, 3)?),
            Path::new(required_arg(args, 4)?),
        )?;
        return Ok(true);
    }
    if args.get(1).map(String::as_str) == Some("--child-poll") {
        child_poll(
            Path::new(required_arg(args, 2)?),
            Path::new(required_arg(args, 3)?),
            Path::new(required_arg(args, 4)?),
        )?;
        return Ok(true);
    }
    if args.get(1).map(String::as_str) == Some("--child-validate") {
        child_validate(
            Path::new(required_arg(args, 2)?),
            Path::new(required_arg(args, 3)?),
            Path::new(required_arg(args, 4)?),
        )?;
        return Ok(true);
    }
    if args.get(1).map(String::as_str) == Some("--poll-once") {
        poll_once(
            required_arg(args, 2)?,
            Path::new(required_arg(args, 3)?),
            Path::new(required_arg(args, 4)?),
        )
        .await?;
        return Ok(true);
    }
    Ok(false)
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    token: String,
    paths: Vec<PathBuf>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = HostState {
        token: Arc::new(token),
        paths: Arc::new(paths),
        limits: HostLimits::from_env(),
        catalog: Arc::new(RwLock::new(BTreeMap::new())),
    };
    reload_catalog(&state).await;
    let host_request_limit = state
        .limits
        .body_bytes
        .saturating_mul(2)
        .saturating_add(64 * 1024);
    let router = Router::new()
        .route("/live", get(live))
        .route("/health", get(health))
        .route("/kinds", get(kinds))
        .route("/reload", post(reload))
        .route("/verify-normalize", post(invoke))
        .route("/validate", post(validate))
        .route("/poll", post(poll))
        .layer(DefaultBodyLimit::max(host_request_limit))
        .with_state(state);
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

async fn poll_once(
    kind: &str,
    request_path: &Path,
    response_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let request: AdapterPollRequest = serde_json::from_slice(&std::fs::read(request_path)?)?;
    let response = if builtin_catalog().contains_key(kind) {
        builtin_poll(kind, request).await
    } else {
        let paths = env::paths("RUNINATOR_ADAPTER_PLUGIN_PATHS");
        let limits = HostLimits::from_env();
        let mut selected = None;
        for directory in paths {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if is_library(&path)
                    && dynamic_metadata(&path, limits)
                        .await
                        .is_ok_and(|metadata| metadata.kind == kind)
                {
                    selected = Some(path);
                    break;
                }
            }
        }
        match selected {
            Some(path) => invoke_dynamic_poll(&path, &request, limits)
                .await
                .unwrap_or_else(|error| AdapterPollResponse {
host_version: None,
kind_version: None,
                    events: Vec::new(),
                    checkpoint: request.checkpoint,
                    retry_after_seconds: None,
                    error: Some(error),
                }),
            None => AdapterPollResponse {
host_version: None,
kind_version: None,
                events: Vec::new(),
                checkpoint: request.checkpoint,
                retry_after_seconds: None,
                error: Some(format!(
                    "adapter kind '{kind}' is not installed on this worker"
                )),
            },
        }
    };
    let response = stamp_host_identity(kind, response);
    std::fs::write(response_path, serde_json::to_vec(&response)?)?;
    Ok(())
}

/// Name the host that answered. Both fields are stamped here rather than at each poller so no
/// response — including the ones built on the error paths — can leave without them.
fn stamp_host_identity(kind: &str, mut response: AdapterPollResponse) -> AdapterPollResponse {
    response.host_version = Some(env!("CARGO_PKG_VERSION").to_owned());
    response.kind_version = builtin_catalog()
        .get(kind)
        .map(|entry| entry.metadata.version.clone());
    response
}

fn required_arg(args: &[String], index: usize) -> Result<&str, Box<dyn std::error::Error>> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing argument {index}").into())
}

/// Unauthenticated liveness. Deliberately reports nothing but that the process is serving: a
/// container probe cannot present the host credential without writing it into the pod spec, and
/// `/health` below discloses the plugin paths and limits, so the two cannot be the same endpoint.
async fn live() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
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
            "plugin_paths": state.paths.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
            "limits": {
                "body_bytes": state.limits.body_bytes,
                "output_bytes": state.limits.output_bytes,
                "event_count": state.limits.event_count,
                "timeout_ms": state.limits.timeout.as_millis(),
            }
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
    if let Err(error) = decode_body_bytes(&request.request, state.limits.body_bytes) {
        let status = if error == "request body exceeds limit" {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::BAD_REQUEST
        };
        return (status, Json(json!({ "error": error })));
    }
    let entry = state.catalog.read().await.get(&request.kind).cloned();
    let Some(entry) = entry else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "adapter kind not found" })),
        );
    };
    let response = if entry.origin == "builtin" {
        builtin_handle(&request.kind, request.request, state.limits.body_bytes)
    } else {
        invoke_dynamic(Path::new(&entry.origin), &request.request, state.limits)
            .await
            .unwrap_or_else(AdapterResponse::rejected)
    };
    if response.events.len() > state.limits.event_count {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "adapter emitted too many events" })),
        );
    }
    let value = serde_json::to_value(response).unwrap_or_default();
    if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() > state.limits.output_bytes) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "adapter output exceeds limit" })),
        );
    }
    (StatusCode::OK, Json(value))
}

async fn poll(
    State(state): State<HostState>,
    headers: HeaderMap,
    Json(request): Json<PollInvokeRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let entry = state.catalog.read().await.get(&request.kind).cloned();
    let Some(entry) = entry else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "adapter kind not found" })),
        );
    };
    let response = if entry.origin == "builtin" {
        builtin_poll(&request.kind, request.request).await
    } else {
        let checkpoint = request.request.checkpoint.clone();
        invoke_dynamic_poll(Path::new(&entry.origin), &request.request, state.limits)
            .await
            .unwrap_or_else(|error| AdapterPollResponse {
host_version: None,
kind_version: None,
                events: Vec::new(),
                checkpoint,
                retry_after_seconds: None,
                error: Some(error),
            })
    };
    if response.events.len() > state.limits.event_count.max(256) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "adapter emitted too many events" })),
        );
    }
    let value = serde_json::to_value(response).unwrap_or_default();
    if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() > state.limits.output_bytes) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "adapter output exceeds limit" })),
        );
    }
    (StatusCode::OK, Json(value))
}

async fn validate(
    State(state): State<HostState>,
    headers: HeaderMap,
    Json(request): Json<ValidateInvokeRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let entry = state.catalog.read().await.get(&request.kind).cloned();
    let Some(entry) = entry else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "adapter kind not found" })),
        );
    };
    let response = if entry.origin == "builtin" {
        builtin_validate(&request.kind, request.request)
    } else {
        invoke_dynamic_validation(Path::new(&entry.origin), &request.request, state.limits)
            .await
            .unwrap_or_else(|error| AdapterValidationResponse {
                issues: vec![runinator_adapter_contract::AdapterValidationIssue {
                    path: String::new(),
                    code: "adapter_validation_failed".into(),
                    message: error,
                    severity: runinator_adapter_contract::AdapterValidationSeverity::Error,
                }],
            })
    };
    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap_or_default()),
    )
}

fn authorized(state: &HostState, headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| verify_bearer(&state.token, value))
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
            match dynamic_metadata(&path, state.limits).await {
                Ok(metadata) => {
                    catalog.insert(
                        metadata.kind.clone(),
                        AdapterKindCatalogEntry {
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
                        AdapterKindCatalogEntry {
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

async fn dynamic_metadata(path: &Path, limits: HostLimits) -> Result<AdapterKindMetadata, String> {
    let temp = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let output = timeout(
        limits.timeout,
        Command::new(adapter_child_executable()?)
            .arg("--child-metadata")
            .arg(path)
            .arg(temp.path())
            .output(),
    )
    .await
    .map_err(|_| "adapter metadata timed out".to_string())?
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("metadata child exited with {}", output.status)
        } else {
            detail
        });
    }
    let bytes = std::fs::read(temp.path()).map_err(|error| error.to_string())?;
    if bytes.len() > limits.output_bytes {
        return Err("metadata output exceeds limit".into());
    }
    let envelope: AdapterMetadataEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if envelope.abi_version != ADAPTER_ABI_VERSION {
        return Err(format!(
            "adapter ABI {} unsupported; rebuild with adapter SDK v{ADAPTER_ABI_VERSION}",
            envelope.abi_version
        ));
    }
    Ok(envelope.metadata)
}

async fn invoke_dynamic(
    path: &Path,
    request: &AdapterRequest,
    limits: HostLimits,
) -> Result<AdapterResponse, String> {
    let request_file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let response_file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let request_bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    if request_bytes.len() > limits.body_bytes {
        return Err("adapter request exceeds limit".into());
    }
    std::fs::write(request_file.path(), request_bytes).map_err(|error| error.to_string())?;
    let mut child = Command::new(adapter_child_executable()?)
        .arg("--child-handle")
        .arg(path)
        .arg(request_file.path())
        .arg(response_file.path())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let status = timeout(limits.timeout, child.wait())
        .await
        .map_err(|_| "adapter invocation timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("adapter child exited with {status}"));
    }
    let bytes = std::fs::read(response_file.path()).map_err(|error| error.to_string())?;
    if bytes.len() > limits.output_bytes {
        return Err("adapter output exceeds limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

async fn invoke_dynamic_poll(
    path: &Path,
    request: &AdapterPollRequest,
    limits: HostLimits,
) -> Result<AdapterPollResponse, String> {
    let request_file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let response_file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let request_bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    if request_bytes.len() > limits.body_bytes {
        return Err("adapter poll request exceeds limit".into());
    }
    std::fs::write(request_file.path(), request_bytes).map_err(|error| error.to_string())?;
    let mut child = Command::new(adapter_child_executable()?)
        .arg("--child-poll")
        .arg(path)
        .arg(request_file.path())
        .arg(response_file.path())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let status = timeout(limits.timeout, child.wait())
        .await
        .map_err(|_| "adapter poll invocation timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("adapter poll child exited with {status}"));
    }
    let bytes = std::fs::read(response_file.path()).map_err(|error| error.to_string())?;
    if bytes.len() > limits.output_bytes {
        return Err("adapter poll output exceeds limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

async fn invoke_dynamic_validation(
    path: &Path,
    request: &AdapterValidationRequest,
    limits: HostLimits,
) -> Result<AdapterValidationResponse, String> {
    let request_file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let response_file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let request_bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    if request_bytes.len() > limits.body_bytes {
        return Err("adapter validation request exceeds limit".into());
    }
    std::fs::write(request_file.path(), request_bytes).map_err(|error| error.to_string())?;
    let mut child = Command::new(adapter_child_executable()?)
        .arg("--child-validate")
        .arg(path)
        .arg(request_file.path())
        .arg(response_file.path())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let status = timeout(limits.timeout, child.wait())
        .await
        .map_err(|_| "adapter validation timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("adapter validation child exited with {status}"));
    }
    let bytes = std::fs::read(response_file.path()).map_err(|error| error.to_string())?;
    if bytes.len() > limits.output_bytes {
        return Err("adapter validation output exceeds limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn adapter_child_executable() -> Result<PathBuf, String> {
    match env::path("RUNINATOR_ADAPTER_CHILD_EXE") {
        Some(path) => Ok(path),
        _ => std::env::current_exe().map_err(|error| error.to_string()),
    }
}

fn child_metadata(
    library_path: &Path,
    response_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    child_file_operation(library_path, METADATA_SYMBOL, None, response_path, true)
}

fn child_handle(
    library_path: &Path,
    request_path: &Path,
    response_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    child_file_operation(
        library_path,
        HANDLE_SYMBOL,
        Some(request_path),
        response_path,
        false,
    )
}

fn child_poll(
    library_path: &Path,
    request_path: &Path,
    response_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    child_file_operation(
        library_path,
        POLL_SYMBOL,
        Some(request_path),
        response_path,
        false,
    )
}

fn child_validate(
    library_path: &Path,
    request_path: &Path,
    response_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    child_file_operation(
        library_path,
        VALIDATE_SYMBOL,
        Some(request_path),
        response_path,
        false,
    )
}

fn child_file_operation(
    library_path: &Path,
    operation_symbol: &[u8],
    request_path: Option<&Path>,
    response_path: &Path,
    validate_name: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: all dynamically loaded code is contained in this disposable child process.
    unsafe {
        let library = libloading::Library::new(library_path)?;
        let version = find_marker(&library, MARKER_SYMBOL, |marker: MarkerFn| marker())?;
        if version != ADAPTER_ABI_VERSION {
            return Err(format!(
                "adapter ABI {version} unsupported; rebuild with adapter SDK v{ADAPTER_ABI_VERSION}"
            )
            .into());
        }
        if validate_name {
            let name = call_symbol(&library, NAME_SYMBOL, |name: NameFn| name())?;
            let _ = cstr_to_rust_string(name)?;
        }
        let operation = library.get::<FileOperationFn>(operation_symbol)?;
        invoke_file_operation(*operation, request_path, response_path)?;
    }
    Ok(())
}

fn builtin_catalog() -> BTreeMap<String, AdapterKindCatalogEntry> {
    builtins::registry()
        .values()
        .map(|adapter| adapter.metadata())
        .map(|metadata| {
            (
                metadata.kind.clone(),
                AdapterKindCatalogEntry {
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
        polling_fields: vec![],
        event_names: vec![],
        canonical_pointers: vec![],
        capabilities: vec![],
        polling_authentication: vec![],
        polling_secret_fields: vec![],
        execution_profile_scopes: vec![],
        execution_profile_required_labels: BTreeMap::new(),
        scope_template: None,
        identity_fields: vec![],
        setup_instructions: vec![],
    }
}

fn field(
    name: &str,
    value_type: RuninatorType,
    required: bool,
    secret: bool,
    description: &str,
    default: Value,
) -> AdapterConfigurationField {
    AdapterConfigurationField {
        name: name.into(),
        value_type,
        required,
        secret,
        description: Some(description.into()),
        default: default.into(),
    }
}

fn sdlc_profile_type() -> RuninatorType {
    let mut profile = RuninatorType::structure([
        (
            "repository",
            RuninatorType::structure([
                ("owner", RuninatorType::String),
                ("name", RuninatorType::String),
                ("remote", RuninatorType::String),
                ("base_branch", RuninatorType::String),
                ("base_ref", RuninatorType::String),
                ("local_path", RuninatorType::String),
                ("github_scope", RuninatorType::String),
            ]),
        ),
        (
            "automation",
            RuninatorType::structure([
                ("branch_prefix", RuninatorType::String),
                ("local_check_command", RuninatorType::String),
                ("merge_method", RuninatorType::String),
            ]),
        ),
        (
            "jira",
            RuninatorType::structure([
                ("base_url", RuninatorType::String),
                ("email", RuninatorType::String),
                ("done_transition_id", RuninatorType::String),
                ("done_status", RuninatorType::String),
            ]),
        ),
        (
            "slack",
            RuninatorType::structure([("search_query", RuninatorType::String)]),
        ),
        (
            "deployment",
            RuninatorType::structure([
                ("workflow_id", RuninatorType::String),
                ("ref", RuninatorType::String),
            ]),
        ),
    ]);
    if let RuninatorType::Struct { fields, .. } = &mut profile {
        // installation-owned project policy travels with the delivery profile but remains opaque
        // to the builtin adapter. workflows can type and consume this region without a host release.
        fields.insert(
            "extensions".into(),
            RuninatorField::optional(RuninatorType::Any),
        );
    }
    profile
}

fn generic_metadata() -> AdapterKindMetadata {
    AdapterKindMetadata {
        kind: "generic_webhook".into(),
        version: "1".into(),
        display_name: "Generic webhook".into(),
        description: Some("HMAC-SHA256 or bearer-authenticated JSON webhook".into()),
        fields: vec![
            field(
                "authentication",
                RuninatorType::Enum(vec!["hmac_sha256".into(), "bearer".into()]),
                true,
                false,
                "Verification scheme used by the sender.",
                "hmac_sha256".into(),
            ),
            field(
                "secret",
                RuninatorType::String,
                true,
                true,
                "Stored Secret used to verify the signature or bearer token.",
                Value::Null,
            ),
            field(
                "delivery_id_pointer",
                RuninatorType::String,
                true,
                false,
                "JSON pointer to a provider-stable delivery identifier.",
                "/delivery_id".into(),
            ),
            field(
                "scope_pointer",
                RuninatorType::String,
                true,
                false,
                "JSON pointer to the admission scope.",
                "/scope".into(),
            ),
            field(
                "correlation_pointer",
                RuninatorType::String,
                true,
                false,
                "JSON pointer to the resource correlation key.",
                "/correlation_key".into(),
            ),
            field(
                "event_pointer",
                RuninatorType::String,
                true,
                false,
                "JSON pointer to the normalized event name.",
                "/event_type".into(),
            ),
            field(
                "occurred_at_pointer",
                RuninatorType::String,
                false,
                false,
                "Optional JSON pointer to an RFC 3339 or epoch occurrence time.",
                Value::Null,
            ),
            field(
                "payload_pointer",
                RuninatorType::String,
                false,
                false,
                "Optional JSON pointer selecting the normalized payload subtree.",
                Value::Null,
            ),
            field(
                "subject_revision_pointer",
                RuninatorType::String,
                false,
                false,
                "Optional JSON pointer to a revision used to fence stale signals.",
                Value::Null,
            ),
            field(
                "provenance_pointer",
                RuninatorType::String,
                false,
                false,
                "Optional JSON pointer to provider-operation provenance.",
                Value::Null,
            ),
        ],
        polling_fields: vec![],
        event_names: vec![],
        canonical_pointers: vec![
            "/delivery_id".into(),
            "/scope".into(),
            "/correlation_key".into(),
            "/event_type".into(),
        ],
        capabilities: vec!["hmac_sha256".into(), "bearer".into()],
        polling_authentication: vec![],
        polling_secret_fields: vec![],
        execution_profile_scopes: vec![],
        execution_profile_required_labels: BTreeMap::new(),
        scope_template: None,
        identity_fields: vec![
            "delivery_id_pointer".into(),
            "scope_pointer".into(),
            "correlation_pointer".into(),
        ],
        setup_instructions: vec![
            "Configure the sender to POST the original JSON bytes to the webhook URL above.".into(),
            "For HMAC-SHA256, send sha256=<hex digest> in X-Runinator-Signature; for bearer authentication, send Authorization: Bearer <token>.".into(),
            "Choose stable delivery, scope, correlation, and event pointers before admitting correlations; identity fields lock after first use.".into(),
        ],
    }
}

fn jira_metadata() -> AdapterKindMetadata {
    AdapterKindMetadata {
        kind: "jira".into(),
        version: "1".into(),
        display_name: "Jira".into(),
        description: Some("Canonical Jira issue, change, and comment events".into()),
        fields: vec![
            field(
                "instance_id",
                RuninatorType::String,
                true,
                false,
                "Stable Jira instance identity, such as the site hostname.",
                Value::Null,
            ),
            field(
                "secret",
                RuninatorType::String,
                true,
                true,
                "Stored Secret expected as the webhook bearer token.",
                Value::Null,
            ),
            field(
                "routing_scope",
                RuninatorType::String,
                false,
                false,
                "Optional orchestration scope override. Use mission.sdlc for label-driven SDLC admission.",
                Value::Null,
            ),
            field(
                "sdlc_profile",
                sdlc_profile_type(),
                false,
                false,
                "Project delivery profile injected into admitted mission payloads.",
                Value::Null,
            ),
        ],
        polling_fields: vec![
            field("instance_id", RuninatorType::String, true, false, "Stable Jira instance identity, such as the site hostname.", Value::Null),
            field("base_url", RuninatorType::String, true, false, "Jira site URL.", Value::Null),
            field("email", RuninatorType::String, true, false, "Jira account email.", Value::Null),
            field("jql", RuninatorType::String, true, false, "JQL selecting issues to poll.", Value::Null),
            field("poll_interval_seconds", RuninatorType::Integer, false, false, "Polling cadence in seconds (30–3600).", 60.into()),
            field("routing_scope", RuninatorType::String, false, false, "Optional orchestration scope override. Use mission.sdlc for label-driven SDLC admission.", Value::Null),
            field("sdlc_profile", sdlc_profile_type(), false, false, "Project delivery profile injected into admitted mission payloads.", Value::Null),
        ],
        event_names: vec!["issue_updated".into(), "comment_created".into()],
        canonical_pointers: vec![
            "/issue/id".into(),
            "/issue/key".into(),
            "/changes".into(),
            "/provenance".into(),
        ],
        capabilities: vec!["bearer".into(), "polling".into()],
        polling_authentication: vec![AdapterAuthenticationKind::Secrets],
        polling_secret_fields: vec![field(
            "api_token",
            RuninatorType::String,
            true,
            true,
            "Jira API token used with the configured account email.",
            Value::Null,
        )],
        execution_profile_scopes: vec![],
        execution_profile_required_labels: BTreeMap::new(),
        scope_template: Some("{routing_scope}".into()),
        identity_fields: vec!["instance_id".into(), "routing_scope".into()],
        setup_instructions: vec![
            "Choose webhook delivery or polling when creating the adapter.".into(),
            "For webhooks, configure Jira automation to POST issue and comment deliveries and send the selected Secret as a bearer token.".into(),
            "For polling, select an API-token Secret and configure the Jira base URL, account email, JQL, and cadence.".into(),
            "Set the stable Jira instance identity and optional routing scope before enabling the adapter; both become part of admitted correlation identity.".into(),
        ],
    }
}

fn slack_ingress_metadata() -> AdapterKindMetadata {
    AdapterKindMetadata {
        kind: "slack_ingress".into(),
        version: "1".into(),
        display_name: "Slack interaction ingress".into(),
        description: Some(
            "Signed Slack replies for explicitly routed actionable notifications".into(),
        ),
        fields: vec![
            field(
                "team_id",
                RuninatorType::String,
                true,
                false,
                "Slack workspace/team id used for correlation and identity mapping.",
                Value::Null,
            ),
            field(
                "signing_secret",
                RuninatorType::String,
                true,
                true,
                "Slack app signing secret used to verify Events API requests.",
                Value::Null,
            ),
        ],
        polling_fields: vec![],
        event_names: vec!["interaction_response".into()],
        canonical_pointers: vec![
            "/team_id".into(),
            "/user_id".into(),
            "/channel".into(),
            "/thread_ts".into(),
            "/text".into(),
        ],
        capabilities: vec!["slack_signing_secret".into(), "interaction_response".into()],
        polling_authentication: vec![],
        polling_secret_fields: vec![],
        execution_profile_scopes: vec![],
        execution_profile_required_labels: BTreeMap::new(),
        scope_template: Some("{channel}".into()),
        identity_fields: vec!["team_id".into()],
        setup_instructions: vec![
            "Create a Slack app, enable Events API, and subscribe to message.channels and message.groups as needed.".into(),
            "Set the Events API request URL to this adapter's webhook URL and bind the app signing secret.".into(),
            "Map Slack users to Runinator users before allowing replies to apply notification actions.".into(),
            "Select Slack explicitly on an interactive notification policy and provide team_id so outbound messages can bind their replies.".into(),
        ],
    }
}

fn github_metadata() -> AdapterKindMetadata {
    AdapterKindMetadata {
        kind: "github".into(),
        version: "1".into(),
        display_name: "GitHub".into(),
        description: Some("Canonical repository, pull request, check, and workflow events".into()),
        fields: vec![field(
            "secret",
            RuninatorType::String,
            true,
            true,
            "Stored Secret used to verify X-Hub-Signature-256.",
            Value::Null,
        )],
        polling_fields: vec![
            field("repositories", RuninatorType::array(RuninatorType::String), true, false, "Repositories to poll in owner/name form.", Value::Null),
            field("poll_interval_seconds", RuninatorType::Integer, false, false, "Polling cadence in seconds (30–3600).", 60.into()),
        ],
        event_names: vec![
            "pull_request".into(),
            "pull_request_review".into(),
            "issue_comment".into(),
            "check_run".into(),
            "workflow_run".into(),
        ],
        canonical_pointers: vec![
            "/repository/id".into(),
            "/pull_request/id".into(),
            "/subject_revision".into(),
            "/provenance".into(),
        ],
        capabilities: vec!["hmac_sha256".into(), "polling".into()],
        polling_authentication: vec![
            AdapterAuthenticationKind::ExecutionProfile,
            AdapterAuthenticationKind::Secrets,
        ],
        polling_secret_fields: vec![field(
            "access_token",
            RuninatorType::String,
            true,
            true,
            "GitHub API token supplied to the GitHub CLI for this adapter.",
            Value::Null,
        )],
        execution_profile_scopes: vec!["github".into()],
        execution_profile_required_labels: BTreeMap::from([("runner".into(), "desktop".into())]),
        scope_template: Some("github:repository:{repository_id}".into()),
        identity_fields: vec!["repositories".into()],
        setup_instructions: vec![
            "Choose webhook delivery or polling when creating the adapter.".into(),
            "For webhooks, use the displayed URL with application/json, select a matching webhook Secret, and subscribe to pull request, pull request review, issue comment, check run, and workflow run events.".into(),
            "For polling, choose either a stored API token or a GitHub execution profile, list repositories as owner/name, and configure the cadence. Both modes invoke the GitHub CLI.".into(),
        ],
    }
}

fn builtin_handle(kind: &str, request: AdapterRequest, body_limit: usize) -> AdapterResponse {
    builtins::registry().get(kind).map_or_else(
        || AdapterResponse::rejected("unknown built-in adapter"),
        |adapter| adapter.handle(request, body_limit),
    )
}

async fn builtin_poll(kind: &str, request: AdapterPollRequest) -> AdapterPollResponse {
    match builtins::registry().get(kind) {
        Some(adapter) => adapter.poll(request).await,
        None => builtins::unsupported_poll(request),
    }
}

fn builtin_validate(kind: &str, request: AdapterValidationRequest) -> AdapterValidationResponse {
    builtins::registry().get(kind).map_or_else(
        || AdapterValidationResponse {
            issues: vec![runinator_adapter_contract::AdapterValidationIssue {
                path: String::new(),
                code: "unknown_adapter_kind".into(),
                message: "unknown built-in adapter".into(),
                severity: runinator_adapter_contract::AdapterValidationSeverity::Error,
            }],
        },
        |adapter| adapter.validate(request),
    )
}

fn poll_secret<'a>(request: &'a AdapterPollRequest, name: &str) -> Result<&'a str, String> {
    configured_string(&request.secrets, name)
        .or_else(|_| configured_string(&request.configuration, name))
}

fn canonical_poll_timestamp(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .or_else(|_| chrono::DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .map(|value| value.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|_| value.to_owned())
}

fn poll_response(events: Vec<NormalizedAdapterEvent>, checkpoint: Value) -> AdapterPollResponse {
    AdapterPollResponse {
host_version: None,
kind_version: None,
        events,
        checkpoint,
        retry_after_seconds: None,
        error: None,
    }
}

/// A service poll failure that preserves its optional upstream retry hint.
struct PollError {
    message: String,
    retry_after_seconds: Option<u64>,
}

impl From<String> for PollError {
    fn from(value: String) -> Self {
        Self {
            message: value,
            retry_after_seconds: None,
        }
    }
}

/// Walk a GitHub collection newest-first, stopping at the first page whose items are all older
/// than `since`. Without this a repository with more than one page of activity silently dropped
/// everything past the first hundred items the moment the watermark moved past them.
async fn github_collect(
    client: &AsyncGitHubClient,
    mut operation: impl FnMut(u32) -> GitHubOperation,
    array_key: Option<&str>,
    since: Option<&str>,
    timestamp_of: impl Fn(&Value) -> String + Copy,
) -> Result<Vec<Value>, PollError> {
    // an operation that ignores `page` answers every page number with the same body, which this
    // walk reads as a full page and requests again: ten identical pages, a thousand duplicate
    // items, and then a hard page-budget failure that retains the checkpoint and stalls the
    // adapter for good. refusing it here is a bug report at the call site instead.
    let probe = operation(1);
    if !probe.paginates() {
        return Err(PollError {
            message: format!(
                "GitHub operation {} is not paginated and cannot be collected page by page; call it directly",
                probe.name()
            ),
            retry_after_seconds: None,
        });
    }
    poll_pages_async(
        Some(1u32),
        GITHUB_MAX_PAGES,
        |page| {
            let page = page.unwrap_or(1);
            let call = operation(page);
            async move {
                let body = client.execute(call).await.map_err(github_poll_error)?;
                let values = array_key
                    .and_then(|key| body.get(key))
                    .and_then(Value::as_array)
                    .or_else(|| body.as_array())
                    .cloned()
                    .unwrap_or_default();
                let exhausted = since.is_some_and(|since| {
                    values.iter().all(|value| {
                        let stamp = timestamp_of(value);
                        !stamp.is_empty() && stamp.as_str() < since
                    })
                });
                let page_len = values.len();
                let next_cursor = (!exhausted && page_len == 100).then_some(page + 1);
                Ok(PollPage {
                    items: values,
                    next_cursor,
                })
            }
        },
        |limit| incomplete_poll("GitHub", limit),
    )
    .await
}

fn require_complete_page(incomplete: bool) -> Result<(), PollError> {
    if incomplete {
        return Err(PollError {
            message: "GitHub scan exceeded its budget before reaching the checkpoint; checkpoint retained"
                .into(),
            retry_after_seconds: None,
        });
    }
    Ok(())
}

fn github_poll_error(error: runinator_github::errors::GitHubError) -> PollError {
    PollError {
        retry_after_seconds: error.retry_after_seconds(),
        message: error.to_string(),
    }
}

fn jira_poll_error(error: runinator_jira::errors::JiraError) -> PollError {
    PollError {
        retry_after_seconds: error.retry_after_seconds(),
        message: error.to_string(),
    }
}

fn incomplete_poll(service: &str, limit: PollLimit) -> PollError {
    PollError {
        message: format!("{service} polling did not complete: {limit:?}"),
        retry_after_seconds: None,
    }
}

fn operation_provenance(payload: &Value) -> Value {
    extract_runinator_operation_key(payload)
        .map(|operation_key| json!({ "operation_key": operation_key }))
        .unwrap_or(Value::Null)
}

fn github_repository_id(repository: &str, value: &Value) -> Result<String, String> {
    value
        .get("id")
        .map(value_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("GitHub repository '{repository}' has no stable id"))
}

/// Correlate a polled GitHub item, or `None` when the item belongs to no pull request and must not
/// be reported. A conversation comment is the only polled kind that can legitimately be unrelated
/// to a pull request, because the issues endpoint also returns plain issue comments.
fn github_poll_correlation(event_type: &str, id: &str, value: &Value) -> Option<String> {
    if event_type == "pull_request" {
        return Some(format!("pr:{id}"));
    }
    if event_type == "issue_comment" {
        // a pull request correlates by number here: the comment carries no pull request id, and
        // `pr-number:` is the alias a mission publishes alongside `pr:` for exactly this case.
        return github_comment_pull_number(value).map(|number| format!("pr-number:{number}"));
    }
    Some(
        value
            .pointer("/pull_requests/0/id")
            .map(|value| format!("pr:{}", value_string(value)))
            .unwrap_or_else(|| format!("workflow:{id}")),
    )
}

/// Read a conversation comment's pull request number from its HTML url, which is the only field on
/// an issue comment that distinguishes a pull request from a plain issue without a second call.
fn github_comment_pull_number(value: &Value) -> Option<String> {
    let html_url = value.get("html_url").and_then(Value::as_str)?;
    let number = html_url
        .split("/pull/")
        .nth(1)?
        .split(['#', '/', '?'])
        .next()?;
    if number.is_empty() || !number.chars().all(|value| value.is_ascii_digit()) {
        return None;
    }
    Some(number.to_owned())
}

fn github_review_stamp(value: &Value) -> String {
    canonical_poll_timestamp(
        value
            .get("submitted_at")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

fn github_updated_at(value: &Value) -> String {
    canonical_poll_timestamp(
        value
            .get("updated_at")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

fn github_check_stamp(value: &Value) -> String {
    canonical_poll_timestamp(
        value
            .get("completed_at")
            .or_else(|| value.get("started_at"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

async fn poll_github(request: AdapterPollRequest) -> AdapterPollResponse {
    run_builtin_poll(request, |request| Box::pin(poll_github_inner(request))).await
}

async fn run_builtin_poll<F>(request: AdapterPollRequest, poll: F) -> AdapterPollResponse
where
    F: for<'a> FnOnce(
        &'a AdapterPollRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AdapterPollResponse, PollError>> + Send + 'a>,
    >,
{
    let fallback_checkpoint = request.checkpoint.clone();
    match poll(&request).await {
        Ok(response) => response,
        Err(error) => AdapterPollResponse {
host_version: None,
kind_version: None,
            events: Vec::new(),
            checkpoint: fallback_checkpoint,
            retry_after_seconds: error.retry_after_seconds,
            error: Some(error.message),
        },
    }
}

async fn poll_github_inner(request: &AdapterPollRequest) -> Result<AdapterPollResponse, PollError> {
    let repositories = request
        .configuration
        .get("repositories")
        .and_then(Value::as_array)
        .ok_or_else(|| "GitHub polling requires configuration.repositories".to_string())?;
    let mut checkpoints = StreamCheckpoints::open(request);
    let mut events = Vec::new();
    let access_token = request
        .secrets
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let client = AsyncGitHubClient::cli(
        access_token.map(str::to_owned),
        Duration::from_secs(30),
        DEFAULT_OUTPUT_LIMIT * 4,
    );

    for repository in repositories.iter().filter_map(Value::as_str) {
        let repository_info = client
            .execute(GitHubOperation::Repository {
                repository: repository.into(),
            })
            .await
            .map_err(github_poll_error)?;
        let repository_id = github_repository_id(repository, &repository_info)?;
        let kinds = [
            "pull_request",
            "workflow_run",
            "check_run",
            "issue_comment",
            "pull_request_review",
        ];
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        // declaring the repository's streams also seeds the ones it has no mark for, which covers
        // both a first poll and a kind added to an adapter that has been running for weeks.
        let seeded = checkpoints.declare(&repository_id, &kinds, &now);

        for (event_type, array_key) in [
            ("pull_request", None),
            ("workflow_run", Some("workflow_runs")),
            ("issue_comment", None),
        ] {
            if seeded.is_seeded(event_type) {
                continue;
            }
            let since = checkpoints.mark(&seeded, event_type);
            for value in github_collect(
                &client,
                |page| match event_type {
                    "pull_request" => GitHubOperation::PullRequests {
                        repository: repository.into(),
                        state: "all".into(),
                        head: None,
                        per_page: 100,
                        page,
                    },
                    "issue_comment" => GitHubOperation::RepositoryIssueComments {
                        repository: repository.into(),
                        per_page: 100,
                        page,
                    },
                    _ => GitHubOperation::WorkflowRuns {
                        repository: repository.into(),
                        workflow_id: None,
                        branch: None,
                        event: None,
                        status: None,
                        per_page: Some(100),
                        page: Some(page),
                    },
                },
                array_key,
                since.as_deref(),
                github_updated_at,
            )
            .await?
            {
                let updated = github_updated_at(&value);
                if since
                    .as_deref()
                    .is_some_and(|mark| !updated.is_empty() && updated.as_str() < mark)
                {
                    continue;
                }
                let id = value.get("id").map(value_string).unwrap_or_default();
                if id.is_empty() {
                    continue;
                }
                let Some(correlation_key) = github_poll_correlation(event_type, &id, &value) else {
                    continue;
                };
                let mut payload = value.clone();
                if let Some(object) = payload.as_object_mut() {
                    object.insert("repository".into(), repository_info.clone());
                    object.insert(event_type.into(), value.clone());
                }
                checkpoints.advance(&seeded, event_type, &updated);
                events.push(NormalizedAdapterEvent {
                    source: "github".into(),
                    delivery_id: format!("github:{repository_id}:{event_type}:{id}:{updated}"),
                    event_type: event_type.into(),
                    scope: format!("github:repository:{repository_id}"),
                    correlation_key,
                    subject_revision: value
                        .get("head_sha")
                        .or_else(|| value.pointer("/head/sha"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    occurred_at: parse_occurred_at(&Value::String(updated)).ok(),
                    provenance: operation_provenance(&payload).into(),
                    payload: payload.into(),
                });
            }
        }

        // reviews have no repository-wide endpoint, so they are collected per pull request. the
        // outer list is bounded by this stream's own mark, which keeps the fan-out to the pull
        // requests that actually moved since the last poll.
        let since = checkpoints.mark(&seeded, "pull_request_review");
        let reviewed_pulls = if seeded.is_seeded("pull_request_review") {
            Vec::new()
        } else {
            github_collect(
                &client,
                |page| GitHubOperation::PullRequests {
                    repository: repository.into(),
                    state: "all".into(),
                    head: None,
                    per_page: 100,
                    page,
                },
                None,
                since.as_deref(),
                github_updated_at,
            )
            .await?
        };
        for pull in reviewed_pulls {
            let pull_id = pull.get("id").map(value_string).unwrap_or_default();
            let number = pull.get("number").map(value_string).unwrap_or_default();
            if pull_id.is_empty() || number.is_empty() {
                continue;
            }
            // the reviews listing is not paginated, so it is fetched whole rather than through
            // `github_collect`; a pull request returns its reviews oldest first, which the
            // newest-first page walk would misread as an exhausted stream.
            let reviews = client
                .execute(GitHubOperation::Reviews {
                    repository: repository.into(),
                    pull_number: number.clone(),
                })
                .await
                .map_err(github_poll_error)?;
            for review in reviews.as_array().cloned().unwrap_or_default() {
                let submitted = github_review_stamp(&review);
                if submitted.is_empty()
                    || since
                        .as_deref()
                        .is_some_and(|mark| submitted.as_str() < mark)
                {
                    continue;
                }
                let review_id = review.get("id").map(value_string).unwrap_or_default();
                if review_id.is_empty() {
                    continue;
                }
                let mut payload = review.clone();
                if let Some(object) = payload.as_object_mut() {
                    object.insert("repository".into(), repository_info.clone());
                    object.insert("pull_request".into(), pull.clone());
                    object.insert("pull_request_review".into(), review.clone());
                }
                checkpoints.advance(&seeded, "pull_request_review", &submitted);
                events.push(NormalizedAdapterEvent {
                    source: "github".into(),
                    delivery_id: format!(
                        "github:{repository_id}:pull_request_review:{review_id}:{submitted}"
                    ),
                    event_type: "pull_request_review".into(),
                    scope: format!("github:repository:{repository_id}"),
                    correlation_key: format!("pr:{pull_id}"),
                    subject_revision: review
                        .get("commit_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    occurred_at: parse_occurred_at(&Value::String(submitted)).ok(),
                    provenance: operation_provenance(&payload).into(),
                    payload: payload.into(),
                });
            }
        }

        if seeded.is_seeded("check_run") {
            continue;
        }
        let since = checkpoints.mark(&seeded, "check_run");
        let commits = client
            .execute(GitHubOperation::Commits {
                repository: repository.into(),
                since: since.clone(),
                per_page: 100,
                page: 1,
            })
            .await
            .map_err(github_poll_error)?;
        // one check-runs request per commit is the expensive part of this poll. bounding it keeps a
        // busy repository from spending the hourly quota in a single pass; `since` above is what
        // keeps the steady-state list short in the first place.
        let commits = commits.as_array().cloned().unwrap_or_default();
        require_complete_page(commits.len() > GITHUB_CHECK_RUN_COMMIT_BUDGET)?;
        for commit in commits {
            let Some(sha) = commit.get("sha").and_then(Value::as_str) else {
                continue;
            };
            let checks = github_collect(
                &client,
                |page| GitHubOperation::CheckRuns {
                    repository: repository.into(),
                    git_ref: sha.into(),
                    per_page: Some(100),
                    page: Some(page),
                },
                Some("check_runs"),
                None,
                github_check_stamp,
            )
            .await?;
            for check in checks {
                let updated = github_check_stamp(&check);
                if since
                    .as_deref()
                    .is_some_and(|mark| !updated.is_empty() && updated.as_str() < mark)
                {
                    continue;
                }
                let id = check.get("id").map(value_string).unwrap_or_default();
                if id.is_empty() {
                    continue;
                }
                checkpoints.advance(&seeded, "check_run", &updated);
                events.push(NormalizedAdapterEvent {
                    source: "github".into(),
                    delivery_id: format!("github:{repository_id}:check_run:{id}:{updated}"),
                    event_type: "check_run".into(),
                    scope: format!("github:repository:{repository_id}"),
                    correlation_key: check
                        .pointer("/pull_requests/0/id")
                        .map(|value| format!("pr:{}", value_string(value)))
                        .unwrap_or_else(|| format!("check:{id}")),
                    subject_revision: Some(sha.to_owned()),
                    occurred_at: parse_occurred_at(&Value::String(updated)).ok(),
                    provenance: operation_provenance(&check).into(),
                    payload: json!({ "repository": repository_info, "check_run": check }).into(),
                });
            }
        }
    }

    if request.initialize {
        // a first poll establishes the high-water mark without replaying history, so every stream
        // that reported anything is stamped at its newest item and no event is emitted.
        events.clear();
    }
    Ok(poll_response(events, checkpoints.into_checkpoint()))
}

/// Bound a JQL query by how long ago the checkpoint was, not by an absolute timestamp.
///
/// Jira interprets an absolute JQL timestamp in the *authenticating account's* timezone, while the
/// checkpoint is UTC. On any non-UTC account that silently shifts the window by the offset and
/// skips hours of updates. A relative bound carries no timezone at all, so it means the same thing
/// on every instance. The margin absorbs clock skew between this host and Jira; the overlap it
/// creates is deduplicated downstream by delivery id.
fn jira_relative_bound(previous: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(previous)
        .or_else(|_| chrono::DateTime::parse_from_str(previous, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .ok()?;
    let elapsed = chrono::Utc::now().signed_duration_since(parsed.with_timezone(&chrono::Utc));
    let minutes = elapsed
        .num_minutes()
        .saturating_add(JIRA_SKEW_MARGIN_MINUTES);
    Some(format!(
        "-{}m",
        minutes.clamp(JIRA_SKEW_MARGIN_MINUTES, JIRA_MAX_LOOKBACK_MINUTES)
    ))
}

async fn poll_jira(request: AdapterPollRequest) -> AdapterPollResponse {
    run_builtin_poll(request, |request| Box::pin(poll_jira_inner(request))).await
}

async fn poll_jira_inner(request: &AdapterPollRequest) -> Result<AdapterPollResponse, PollError> {
    let base_url = configured_string(&request.configuration, "base_url")?.trim_end_matches('/');
    let email = configured_string(&request.configuration, "email")?;
    let token = poll_secret(request, "api_token")?;
    let instance_id = configured_string(&request.configuration, "instance_id")?;
    let jql = configured_string(&request.configuration, "jql")?;
    let routing_scope = configured_routing_scope(&request.configuration);

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut checkpoints = StreamCheckpoints::open(request);
    let streams = checkpoints.declare(instance_id, &["issue", "comment"], &now);
    let previous = checkpoints.mark(&streams, "issue");
    let comment_previous = checkpoints.mark(&streams, "comment");
    let query = match previous.as_deref().and_then(jira_relative_bound) {
        Some(bound) => format!("({jql}) AND updated >= \"{bound}\""),
        None => jql.to_owned(),
    };

    let client = AsyncJiraClient::new(
        base_url,
        JiraCredentials {
            email: email.into(),
            token: token.into(),
        },
        Duration::from_secs(30),
    )
    .map_err(jira_poll_error)?;
    let issues = poll_pages_async(None, JIRA_MAX_PAGES, |next_page_token| {
        let operation = JiraOperation::Search {
            jql: query.clone(),
            max_results: Some(100),
            fields: "summary,description,project,status,labels,issuetype,priority,assignee,components,updated,comment".into(),
            next_page_token,
        };
        let client = &client;
        async move {
            let response = client.execute(operation).await.map_err(jira_poll_error)?;
            let items = response.get("issues").and_then(Value::as_array).cloned().unwrap_or_default();
            let next_cursor = response.get("nextPageToken").and_then(Value::as_str).map(str::to_owned).filter(|value| !value.is_empty());
            Ok(PollPage { items, next_cursor })
        }
    }, |limit| incomplete_poll("Jira", limit)).await?;

    let mut events = Vec::new();
    for issue in issues {
        let updated = canonical_poll_timestamp(
            issue
                .pointer("/fields/updated")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let issue_id = issue.get("id").map(value_string).unwrap_or_default();
        let project = issue
            .pointer("/fields/project/id")
            .map(value_string)
            .unwrap_or_default();
        if issue_id.is_empty() || project.is_empty() {
            continue;
        }
        let (scope, correlation_key) = jira_routing_identity(
            routing_scope,
            instance_id,
            project.as_str(),
            issue_id.as_str(),
        );
        if !streams.is_seeded("issue") {
            checkpoints.advance(&streams, "issue", &updated);
            events.push(NormalizedAdapterEvent {
            source: "jira".into(),
            delivery_id: format!("jira:{instance_id}:issue:{issue_id}:{updated}"),
            event_type: "issue_updated".into(),
            scope: scope.clone(),
            correlation_key: correlation_key.clone(),
            subject_revision: None,
            occurred_at: parse_occurred_at(&Value::String(updated.clone())).ok(),
            provenance: operation_provenance(&issue).into(),
            payload: jira_payload_with_profile(
                json!({ "issue": issue.clone() }),
                &request.configuration,
            )
            .into(),
            });
        }
        if streams.is_seeded("comment") {
            continue;
        }
        for comment in issue
            .pointer("/fields/comment/comments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let comment_updated = canonical_poll_timestamp(
                comment
                    .get("updated")
                    .and_then(Value::as_str)
                    .unwrap_or(&updated),
            );
            if comment_previous
                .as_deref()
                .is_some_and(|mark| comment_updated.as_str() < mark)
            {
                continue;
            }
            checkpoints.advance(&streams, "comment", &comment_updated);
            events.push(NormalizedAdapterEvent {
                source: "jira".into(),
                delivery_id: format!(
                    "jira:{instance_id}:comment:{}:{comment_updated}",
                    comment.get("id").map(value_string).unwrap_or_default()
                ),
                event_type: "comment_created".into(),
                scope: scope.clone(),
                correlation_key: correlation_key.clone(),
                subject_revision: None,
                occurred_at: parse_occurred_at(&Value::String(comment_updated)).ok(),
                provenance: operation_provenance(&comment).into(),
                payload: jira_payload_with_profile(
                    json!({ "issue": issue, "comment": comment }),
                    &request.configuration,
                )
                .into(),
            });
        }
    }
    if request.initialize {
        events.clear();
    }
    Ok(poll_response(events, checkpoints.into_checkpoint()))
}

fn decode_body(request: &AdapterRequest, body_limit: usize) -> Result<(Vec<u8>, Value), String> {
    let bytes = decode_body_bytes(request, body_limit)?;
    let json = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok((bytes, json))
}

fn decode_body_bytes(request: &AdapterRequest, body_limit: usize) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&request.body_base64)
        .map_err(|_| "request body is not valid base64".to_string())?;
    if bytes.len() > body_limit {
        return Err("request body exceeds limit".into());
    }
    Ok(bytes)
}

fn configured_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing configuration '{key}'"))
}

fn configured_routing_scope(configuration: &Value) -> Option<&str> {
    configuration
        .get("routing_scope")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn jira_routing_identity(
    routing_scope: Option<&str>,
    instance_id: &str,
    project_id: &str,
    issue_id: &str,
) -> (String, String) {
    match routing_scope {
        Some(scope) => (
            scope.to_owned(),
            format!("jira:{instance_id}:issue:{issue_id}"),
        ),
        None => (
            format!("jira:{instance_id}:project:{project_id}"),
            format!("issue:{issue_id}"),
        ),
    }
}

fn jira_payload_with_profile(mut payload: Value, configuration: &Value) -> Value {
    if let Some(profile) = configuration.get("sdlc_profile")
        && !profile.is_null()
        && let Some(payload) = payload.as_object_mut()
    {
        payload.insert("profile".into(), profile.clone());
    }
    payload
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

fn optional_pointer<'a>(configuration: &'a Value, key: &str) -> Option<&'a str> {
    configuration.get(key).and_then(Value::as_str)
}

fn parse_occurred_at(value: &Value) -> Result<chrono::DateTime<chrono::Utc>, String> {
    use chrono::Utc;

    if let Some(value) = value.as_str() {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(value) {
            return Ok(parsed.with_timezone(&Utc));
        }
        if let Ok(timestamp) = value.parse::<i64>() {
            return time::from_unix_millis_or_seconds(timestamp)
                .ok_or_else(|| "occurrence time is outside the supported range".into());
        }
        return Err("occurrence time is neither RFC3339 nor an epoch timestamp".into());
    }
    value
        .as_i64()
        .ok_or_else(|| "occurrence time must be a string or integer".into())
        .and_then(|timestamp| {
            time::from_unix_millis_or_seconds(timestamp)
                .ok_or_else(|| "occurrence time is outside the supported range".into())
        })
}

fn configured_occurred_at(
    payload: &Value,
    configuration: &Value,
    key: &str,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    let Some(pointer) = optional_pointer(configuration, key) else {
        return Ok(None);
    };
    let value = payload
        .pointer(pointer)
        .ok_or_else(|| format!("pointer '{pointer}' did not resolve"))?;
    parse_occurred_at(value).map(Some)
}

fn first_occurred_at(payload: &Value, pointers: &[&str]) -> Option<chrono::DateTime<chrono::Utc>> {
    pointers
        .iter()
        .filter_map(|pointer| payload.pointer(pointer))
        .find_map(|value| parse_occurred_at(value).ok())
}

fn handle_generic(request: AdapterRequest, body_limit: usize) -> AdapterResponse {
    let Ok((bytes, payload)) = decode_body(&request, body_limit) else {
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
            .is_some_and(|value| verify_hmac_sha256(secret, &bytes, value)),
        "bearer" => request
            .headers
            .get("authorization")
            .is_some_and(|value| verify_bearer(secret, value)),
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
        let provenance = optional_pointer(&request.configuration, "provenance_pointer")
            .and_then(|pointer| payload.pointer(pointer))
            .cloned()
            .unwrap_or(Value::Null);
        let occurred_at =
            configured_occurred_at(&payload, &request.configuration, "occurred_at_pointer")?;
        let normalized_payload = match optional_pointer(&request.configuration, "payload_pointer") {
            Some(pointer) => payload
                .pointer(pointer)
                .cloned()
                .ok_or_else(|| format!("pointer '{pointer}' did not resolve"))?,
            None => payload.clone(),
        };
        Ok::<_, String>(NormalizedAdapterEvent {
            source: "generic_webhook".into(),
            delivery_id,
            event_type,
            scope,
            correlation_key,
            subject_revision: optional_pointer(&request.configuration, "subject_revision_pointer")
                .and_then(|pointer| pointer_string(&payload, pointer).ok()),
            occurred_at,
            payload: normalized_payload.into(),
            provenance: provenance.into(),
        })
    })();
    match result {
        Ok(event) => AdapterResponse {
            verified: true,
            events: vec![event],
            errors: vec![],
            immediate_response: None,
        },
        Err(error) => AdapterResponse::rejected(error),
    }
}

fn handle_github(request: AdapterRequest, body_limit: usize) -> AdapterResponse {
    let Ok((bytes, payload)) = decode_body(&request, body_limit) else {
        return AdapterResponse::rejected("invalid GitHub JSON body");
    };
    let secret = configured_string(&request.secrets, "secret")
        .or_else(|_| configured_string(&request.configuration, "secret"));
    let signature = request.headers.get("x-hub-signature-256");
    if secret
        .ok()
        .zip(signature)
        .is_none_or(|(secret, signature)| !verify_hmac_sha256(secret, &bytes, signature))
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
    if repository.is_empty() {
        return AdapterResponse::rejected("GitHub event lacks repository identity");
    }
    let correlation_key = payload
        .pointer("/pull_request/id")
        .or_else(|| payload.pointer("/check_run/pull_requests/0/id"))
        .or_else(|| payload.pointer("/workflow_run/pull_requests/0/id"))
        .map(|value| format!("pr:{}", value_string(value)))
        .or_else(|| {
            payload
                .pointer("/issue/pull_request")
                .zip(payload.pointer("/issue/number"))
                .map(|(_, value)| format!("pr-number:{}", value_string(value)))
        })
        .or_else(|| {
            payload
                .pointer("/check_run/id")
                .map(|value| format!("check:{}", value_string(value)))
        })
        .or_else(|| {
            payload
                .pointer("/workflow_run/id")
                .map(|value| format!("workflow:{}", value_string(value)))
        })
        .unwrap_or_else(|| format!("repository:{repository}"));
    let subject_revision = payload
        .pointer("/pull_request/head/sha")
        .or_else(|| payload.pointer("/check_run/head_sha"))
        .or_else(|| payload.pointer("/workflow_run/head_sha"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let provenance = operation_provenance(&payload);
    AdapterResponse {
        verified: true,
        events: vec![NormalizedAdapterEvent {
            source: "github".into(),
            delivery_id,
            event_type,
            scope: format!("github:repository:{repository}"),
            correlation_key,
            subject_revision,
            occurred_at: first_occurred_at(
                &payload,
                &[
                    "/pull_request/updated_at",
                    "/review/submitted_at",
                    "/comment/updated_at",
                    "/comment/created_at",
                    "/check_run/completed_at",
                    "/check_run/started_at",
                    "/workflow_run/updated_at",
                ],
            ),
            payload: payload.into(),
            provenance: provenance.into(),
        }],
        errors: vec![],
        immediate_response: None,
    }
}

fn handle_jira(request: AdapterRequest, body_limit: usize) -> AdapterResponse {
    let Ok((_, payload)) = decode_body(&request, body_limit) else {
        return AdapterResponse::rejected("invalid Jira JSON body");
    };
    let secret = configured_string(&request.secrets, "secret")
        .or_else(|_| configured_string(&request.configuration, "secret"));
    let verified = secret.ok().is_some_and(|secret| {
        request
            .headers
            .get("authorization")
            .is_some_and(|value| verify_bearer(secret, value))
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
        .strip_prefix("jira:")
        .unwrap_or_else(|| payload["webhookEvent"].as_str().unwrap_or("jira_event"))
        .to_owned();
    let instance_id = match configured_string(&request.configuration, "instance_id") {
        Ok(value) if !value.trim().is_empty() => value.trim(),
        _ => return AdapterResponse::rejected("Jira instance identity is required"),
    };
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
    if delivery_id.is_empty() || issue_id.is_empty() || project_id.is_empty() {
        return AdapterResponse::rejected(
            "Jira event lacks stable delivery, project, or issue identity",
        );
    }
    let (scope, correlation_key) = jira_routing_identity(
        configured_routing_scope(&request.configuration),
        instance_id,
        &project_id,
        &issue_id,
    );
    let provenance = operation_provenance(&payload);
    let occurred_at = first_occurred_at(&payload, &["/timestamp"]);
    let payload = jira_payload_with_profile(payload, &request.configuration);
    AdapterResponse {
        verified: true,
        events: vec![NormalizedAdapterEvent {
            source: "jira".into(),
            delivery_id,
            event_type,
            scope,
            correlation_key,
            subject_revision: None,
            occurred_at,
            payload: payload.into(),
            provenance: provenance.into(),
        }],
        errors: vec![],
        immediate_response: None,
    }
}

fn handle_slack_ingress(request: AdapterRequest, body_limit: usize) -> AdapterResponse {
    let Ok((bytes, payload)) = decode_body(&request, body_limit) else {
        return AdapterResponse::rejected("invalid Slack JSON body");
    };
    let timestamp = request
        .headers
        .get("x-slack-request-timestamp")
        .and_then(|value| value.parse::<i64>().ok());
    let Some(timestamp) = timestamp else {
        return AdapterResponse::rejected("missing Slack request timestamp");
    };
    if (chrono::Utc::now().timestamp() - timestamp).abs() > 300 {
        return AdapterResponse::rejected("Slack request timestamp is outside the replay window");
    }
    let signature = request.headers.get("x-slack-signature");
    let secret = configured_string(&request.secrets, "signing_secret")
        .or_else(|_| configured_string(&request.configuration, "signing_secret"));
    let mut signed = format!("v0:{timestamp}:").into_bytes();
    signed.extend_from_slice(&bytes);
    if secret
        .ok()
        .zip(signature)
        .is_none_or(|(secret, signature)| {
            !signature
                .strip_prefix("v0=")
                .is_some_and(|signature| verify_hmac_sha256(secret, &signed, signature))
        })
    {
        return AdapterResponse::rejected("Slack signature verification failed");
    }

    if payload.get("type").and_then(Value::as_str) == Some("url_verification") {
        let Some(challenge) = payload.get("challenge").and_then(Value::as_str) else {
            return AdapterResponse::rejected("Slack URL verification lacks a challenge");
        };
        return AdapterResponse {
            verified: true,
            events: vec![],
            errors: vec![],
            immediate_response: Some(AdapterImmediateResponse {
                status: 200,
                body: json!({ "challenge": challenge }),
            }),
        };
    }

    let configured_team = configured_string(&request.configuration, "team_id").unwrap_or_default();
    let team_id = payload
        .get("team_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if team_id.is_empty() || team_id != configured_team {
        return AdapterResponse::rejected("Slack team id does not match this adapter");
    }
    let event = payload.get("event").unwrap_or(&Value::Null);
    if event.get("type").and_then(Value::as_str) != Some("message")
        || event.get("subtype").is_some()
        || event.get("bot_id").is_some()
    {
        return AdapterResponse {
            verified: true,
            events: vec![],
            errors: vec![],
            immediate_response: None,
        };
    }
    let user_id = event
        .get("user")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let channel = event
        .get("channel")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let thread_ts = event
        .get("thread_ts")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = event
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if user_id.is_empty() || channel.is_empty() || thread_ts.is_empty() || text.trim().is_empty() {
        return AdapterResponse {
            verified: true,
            events: vec![],
            errors: vec![],
            immediate_response: None,
        };
    }
    let command = match text.trim().to_ascii_lowercase().as_str() {
        "approve" | "/approve" => "approve",
        "reject" | "/reject" => "reject",
        _ => "steer",
    };
    let input = if command == "steer" {
        Value::String(text.to_string())
    } else {
        Value::Null
    };
    let delivery_id = payload
        .get("event_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if delivery_id.is_empty() {
        return AdapterResponse::rejected("Slack event lacks an event id");
    }
    AdapterResponse {
        verified: true,
        events: vec![NormalizedAdapterEvent {
            source: format!("slack:{team_id}"),
            delivery_id: delivery_id.into(),
            event_type: "interaction_response".into(),
            scope: channel.into(),
            correlation_key: thread_ts.into(),
            subject_revision: None,
            occurred_at: None,
            payload: json!({
                "team_id": team_id,
                "actor_subject": user_id,
                "channel": channel,
                "thread_ts": thread_ts,
                "message_id": event.get("ts").cloned().unwrap_or(Value::Null),
                "action_id": command,
                "input": input,
            })
            .into(),
            provenance: json!({ "provider": "slack", "event_id": delivery_id }).into(),
        }],
        errors: vec![],
        immediate_response: None,
    }
}

fn value_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
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
    use hmac::{Hmac, KeyInit, Mac};

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

    fn slack_signature(secret: &str, timestamp: i64, body: &[u8]) -> String {
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("v0:{timestamp}:").as_bytes());
        mac.update(body);
        format!(
            "v0={}",
            mac.finalize()
                .into_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }

    #[test]
    fn slack_thread_reply_is_verified_and_normalized_as_an_interaction_response() {
        let timestamp = chrono::Utc::now().timestamp();
        let body = br#"{"type":"event_callback","team_id":"T123","event_id":"Ev123","event":{"type":"message","user":"U123","text":"Please focus on the failing parser test","channel":"C123","ts":"1700.2","thread_ts":"1700.1"}}"#;
        let response = handle_slack_ingress(
            AdapterRequest {
                method: "POST".into(),
                headers: BTreeMap::from([
                    ("x-slack-request-timestamp".into(), timestamp.to_string()),
                    (
                        "x-slack-signature".into(),
                        slack_signature("secret", timestamp, body),
                    ),
                ]),
                body_base64: base64::engine::general_purpose::STANDARD.encode(body),
                configuration: json!({ "team_id": "T123" }),
                secrets: json!({ "signing_secret": "secret" }),
            },
            DEFAULT_BODY_LIMIT,
        );

        assert!(response.verified);
        assert_eq!(response.events[0].source, "slack:T123");
        assert_eq!(response.events[0].scope, "C123");
        assert_eq!(response.events[0].correlation_key, "1700.1");
        assert_eq!(response.events[0].event_type, "interaction_response");
        assert_eq!(
            response.events[0].payload["action_id"].as_str(),
            Some("steer")
        );
        assert_eq!(
            response.events[0].payload["actor_subject"].as_str(),
            Some("U123")
        );
    }

    #[test]
    fn generic_webhook_verifies_and_normalizes_configured_identity() {
        let body = br#"{"delivery":"d-1","tenant":"acme","object":{"id":"42","revision":"abc"},"event":"changed","occurred_at":"2026-08-27T12:34:56Z","data":{"value":7},"origin":{"operation_key":"operation-1"}}"#;
        let response = handle_generic(
            AdapterRequest {
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
                    "event_pointer": "/event",
                    "occurred_at_pointer": "/occurred_at",
                    "payload_pointer": "/data",
                    "subject_revision_pointer": "/object/revision",
                    "provenance_pointer": "/origin"
                }),
                secrets: json!({ "secret": "test-secret" }),
            },
            DEFAULT_BODY_LIMIT,
        );
        assert!(response.verified);
        assert_eq!(response.events[0].delivery_id, "d-1");
        assert_eq!(response.events[0].scope, "acme");
        assert_eq!(response.events[0].correlation_key, "42");
        assert_eq!(response.events[0].subject_revision.as_deref(), Some("abc"));
        assert_eq!(
            response.events[0].occurred_at.unwrap().to_rfc3339(),
            "2026-08-27T12:34:56+00:00"
        );
        assert_eq!(
            serde_json::to_value(&response.events[0].payload).unwrap(),
            json!({ "value": 7 })
        );
        assert_eq!(
            serde_json::to_value(&response.events[0].provenance).unwrap(),
            json!({ "operation_key": "operation-1" })
        );
    }

    #[test]
    fn github_rejects_a_signature_for_different_bytes() {
        let body = br#"{"repository":{"id":1}}"#;
        let response = handle_github(
            AdapterRequest {
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
            },
            DEFAULT_BODY_LIMIT,
        );
        assert!(!response.verified);
        assert!(response.events.is_empty());
    }

    #[test]
    fn github_pr_and_check_events_share_a_correlation() {
        let pull_request_body = br#"{"repository":{"id":10},"pull_request":{"id":20,"head":{"sha":"abc"},"updated_at":"2026-08-27T12:00:00Z"}}"#;
        let check_body = br#"{"repository":{"id":10},"check_run":{"id":30,"head_sha":"abc","pull_requests":[{"id":20}],"completed_at":"2026-08-27T12:05:00Z"}}"#;
        let normalize = |body: &[u8], event: &str| {
            handle_github(
                AdapterRequest {
                    method: "POST".into(),
                    headers: BTreeMap::from([
                        ("x-hub-signature-256".into(), signature("secret", body)),
                        ("x-github-delivery".into(), format!("{event}-delivery")),
                        ("x-github-event".into(), event.into()),
                    ]),
                    body_base64: base64::engine::general_purpose::STANDARD.encode(body),
                    configuration: Value::Null,
                    secrets: json!({ "secret": "secret" }),
                },
                DEFAULT_BODY_LIMIT,
            )
        };
        let pull_request = normalize(pull_request_body, "pull_request");
        let check = normalize(check_body, "check_run");
        assert!(pull_request.verified && check.verified);
        assert_eq!(pull_request.events[0].scope, "github:repository:10");
        assert_eq!(pull_request.events[0].correlation_key, "pr:20");
        assert_eq!(
            check.events[0].correlation_key,
            pull_request.events[0].correlation_key
        );
        assert_eq!(check.events[0].subject_revision.as_deref(), Some("abc"));
    }

    #[test]
    fn polled_conversation_comments_correlate_only_for_pull_requests() {
        let comment = |html_url: &str| json!({ "id": 7, "html_url": html_url });
        assert_eq!(
            github_comment_pull_number(&comment(
                "https://github.com/whiskerlabs/flint/pull/266#issuecomment-31"
            ))
            .as_deref(),
            Some("266")
        );
        // the issues endpoint also returns plain issue comments, which belong to no pull request
        // and must not be reported under a `pr-number:` correlation another mission may own.
        assert_eq!(
            github_comment_pull_number(&comment(
                "https://github.com/whiskerlabs/flint/issues/266#issuecomment-31"
            )),
            None
        );
        assert_eq!(
            github_comment_pull_number(&comment("https://github.com/whiskerlabs/flint/pull/abc")),
            None
        );
        assert_eq!(github_comment_pull_number(&json!({ "id": 7 })), None);
    }

    #[test]
    fn polled_correlations_match_the_aliases_a_mission_publishes() {
        let pull_comment = json!({
            "id": 31,
            "html_url": "https://github.com/whiskerlabs/flint/pull/266#issuecomment-31"
        });
        assert_eq!(
            github_poll_correlation("issue_comment", "31", &pull_comment).as_deref(),
            Some("pr-number:266")
        );
        assert_eq!(
            github_poll_correlation("pull_request", "20", &json!({ "id": 20 })).as_deref(),
            Some("pr:20")
        );
        assert_eq!(
            github_poll_correlation(
                "workflow_run",
                "40",
                &json!({ "pull_requests": [{ "id": 20 }] })
            )
            .as_deref(),
            Some("pr:20")
        );
        assert_eq!(
            github_poll_correlation(
                "issue_comment",
                "31",
                &json!({ "html_url": "https://github.com/whiskerlabs/flint/issues/9" })
            ),
            None
        );
    }

    #[tokio::test]
    async fn a_non_paginating_operation_is_refused_rather_than_walked() {
        // `Reviews` ignores `page`, so walking it returned ten identical pages, about a thousand
        // duplicate items, and then a page-budget failure that retained the checkpoint and stalled
        // the adapter permanently. the refusal costs one request and names the operation.
        let client = AsyncGitHubClient::cli(None, Duration::from_secs(1), DEFAULT_OUTPUT_LIMIT);
        let error = github_collect(
            &client,
            |_page| GitHubOperation::Reviews {
                repository: "owner/repo".into(),
                pull_number: "1".into(),
            },
            None,
            None,
            github_review_stamp,
        )
        .await
        .expect_err("an unpaginated operation must not be collected");
        assert!(error.message.contains("Reviews"), "{}", error.message);
        assert!(error.message.contains("not paginated"), "{}", error.message);
    }

    #[test]
    fn a_paginated_operation_is_accepted_by_the_collector() {
        // the guard has to stay honest about the operations the poller actually walks.
        for operation in [
            GitHubOperation::PullRequests {
                repository: "owner/repo".into(),
                state: "all".into(),
                head: None,
                per_page: 100,
                page: 1,
            },
            GitHubOperation::RepositoryIssueComments {
                repository: "owner/repo".into(),
                per_page: 100,
                page: 1,
            },
            GitHubOperation::WorkflowRuns {
                repository: "owner/repo".into(),
                workflow_id: None,
                branch: None,
                event: None,
                status: None,
                per_page: Some(100),
                page: Some(1),
            },
            GitHubOperation::CheckRuns {
                repository: "owner/repo".into(),
                git_ref: "sha".into(),
                per_page: Some(100),
                page: Some(1),
            },
            GitHubOperation::Commits {
                repository: "owner/repo".into(),
                since: None,
                per_page: 100,
                page: 1,
            },
        ] {
            assert!(operation.paginates(), "{} must paginate", operation.name());
        }
        assert!(
            !GitHubOperation::Reviews {
                repository: "owner/repo".into(),
                pull_number: "1".into(),
            }
            .paginates()
        );
    }

    #[test]
    fn a_review_stamp_reads_its_submission_time() {
        assert_eq!(
            github_review_stamp(&json!({ "submitted_at": "2026-08-27T12:00:00Z" })),
            canonical_poll_timestamp("2026-08-27T12:00:00Z")
        );
        assert!(github_review_stamp(&json!({ "id": 1 })).is_empty());
    }

    #[test]
    fn github_review_and_pr_comment_events_route_back_to_the_pull_request() {
        let review_body = br#"{"repository":{"id":10},"pull_request":{"id":20,"head":{"sha":"abc"}},"review":{"submitted_at":"2026-08-27T12:05:00Z"}}"#;
        let comment_body = br#"{"repository":{"id":10},"issue":{"number":7,"pull_request":{"url":"https://api.github.test/pulls/7"}},"comment":{"created_at":"2026-08-27T12:06:00Z"}}"#;
        let normalize = |body: &[u8], event: &str| {
            handle_github(
                AdapterRequest {
                    method: "POST".into(),
                    headers: BTreeMap::from([
                        ("x-hub-signature-256".into(), signature("secret", body)),
                        ("x-github-delivery".into(), format!("{event}-delivery")),
                        ("x-github-event".into(), event.into()),
                    ]),
                    body_base64: base64::engine::general_purpose::STANDARD.encode(body),
                    configuration: Value::Null,
                    secrets: json!({ "secret": "secret" }),
                },
                DEFAULT_BODY_LIMIT,
            )
        };

        let review = normalize(review_body, "pull_request_review");
        let comment = normalize(comment_body, "issue_comment");
        assert_eq!(review.events[0].correlation_key, "pr:20");
        assert_eq!(review.events[0].subject_revision.as_deref(), Some("abc"));
        assert_eq!(comment.events[0].correlation_key, "pr-number:7");
        assert!(review.events[0].occurred_at.is_some());
        assert!(comment.events[0].occurred_at.is_some());
    }

    #[test]
    fn github_polling_uses_the_same_stable_scope_and_correlation_as_webhooks() {
        let repository = json!({ "id": 10, "full_name": "octo/example" });
        assert_eq!(
            github_repository_id("octo/example", &repository).unwrap(),
            "10"
        );
        assert_eq!(
            github_poll_correlation("pull_request", "20", &json!({ "id": 20 })).as_deref(),
            Some("pr:20")
        );
        assert_eq!(
            github_poll_correlation(
                "workflow_run",
                "30",
                &json!({ "pull_requests": [{ "id": 20 }] }),
            )
            .as_deref(),
            Some("pr:20")
        );
    }

    #[tokio::test]
    async fn liveness_is_unauthenticated_and_discloses_nothing() {
        // this endpoint exists so a container probe can check the sidecar without the host
        // credential being written into the pod spec. that only stays safe while it reports
        // nothing: `/health` keeps the catalog, plugin paths, and limits behind the bearer.
        let (status, body) = live().await;
        assert_eq!(status, StatusCode::OK);
        let body = body.0;
        assert_eq!(body, json!({ "status": "ok" }));
        for leaked in ["plugin_paths", "limits", "kinds", "healthy"] {
            assert!(
                body.get(leaked).is_none(),
                "liveness must not disclose {leaked}; it is served without authentication"
            );
        }
    }

    #[tokio::test]
    async fn generic_builtin_poll_preserves_checkpoint_and_retry_on_failure() {
        let request = AdapterPollRequest {
            configuration: Value::Null,
            secrets: Value::Null,
            checkpoint: json!({ "cursor": "old" }),
            initialize: false,
        };
        let response = run_builtin_poll(request, |_| {
            Box::pin(async {
                Err(PollError {
                    message: "limited".into(),
                    retry_after_seconds: Some(17),
                })
            })
        })
        .await;
        assert_eq!(response.checkpoint, json!({ "cursor": "old" }));
        assert_eq!(response.retry_after_seconds, Some(17));
        assert_eq!(response.error.as_deref(), Some("limited"));
    }

    #[test]
    fn jira_checkpoint_is_bounded_relatively_so_no_timezone_can_shift_it() {
        // an absolute JQL timestamp is read in the jira account's timezone while the checkpoint is
        // utc, so the bound is expressed as an offset from now instead. two checkpoints an hour
        // apart must therefore differ by about an hour of lookback, whatever zone either side is in.
        let recent = chrono::Utc::now() - chrono::TimeDelta::minutes(30);
        let bound = jira_relative_bound(&recent.to_rfc3339()).expect("a bound for a valid stamp");
        let minutes: i64 = bound
            .trim_start_matches('-')
            .trim_end_matches('m')
            .parse()
            .expect("relative minutes");
        assert!(
            (33..=38).contains(&minutes),
            "expected ~30m plus the skew margin, got {bound}"
        );

        // the same instant written in a non-utc offset must produce the same lookback.
        let offset = recent.with_timezone(&chrono::FixedOffset::east_opt(5 * 3600).unwrap());
        assert_eq!(jira_relative_bound(&offset.to_rfc3339()), Some(bound));

        assert_eq!(jira_relative_bound("not a timestamp"), None);
    }

    #[test]
    fn jira_lookback_is_clamped_to_the_supported_window() {
        let ancient = chrono::Utc::now() - chrono::TimeDelta::days(365);
        assert_eq!(
            jira_relative_bound(&ancient.to_rfc3339()),
            Some(format!("-{JIRA_MAX_LOOKBACK_MINUTES}m"))
        );
    }

    #[test]
    fn jira_identity_includes_the_instance_and_project() {
        let body = br#"{"timestamp":1787832000000,"webhookEvent":"jira:issue_updated","issue":{"id":"20","fields":{"project":{"id":"10"}}}}"#;
        let response = handle_jira(
            AdapterRequest {
                method: "POST".into(),
                headers: BTreeMap::from([
                    ("authorization".into(), "Bearer secret".into()),
                    ("x-atlassian-webhook-identifier".into(), "delivery".into()),
                ]),
                body_base64: base64::engine::general_purpose::STANDARD.encode(body),
                configuration: json!({ "instance_id": "acme.atlassian.net" }),
                secrets: json!({ "secret": "secret" }),
            },
            DEFAULT_BODY_LIMIT,
        );
        assert!(response.verified);
        assert_eq!(response.events[0].event_type, "issue_updated");
        assert_eq!(
            response.events[0].scope,
            "jira:acme.atlassian.net:project:10"
        );
        assert_eq!(response.events[0].correlation_key, "issue:20");
        assert!(response.events[0].occurred_at.is_some());
    }

    #[test]
    fn jira_routing_scope_admits_one_mission_per_instance_issue() {
        let body = br#"{"timestamp":1787832000000,"webhookEvent":"jira:issue_updated","issue":{"id":"20","fields":{"project":{"id":"10"}}}}"#;
        let response = handle_jira(
            AdapterRequest {
                method: "POST".into(),
                headers: BTreeMap::from([
                    ("authorization".into(), "Bearer secret".into()),
                    ("x-atlassian-webhook-identifier".into(), "delivery".into()),
                ]),
                body_base64: base64::engine::general_purpose::STANDARD.encode(body),
                configuration: json!({
                    "instance_id": "acme.atlassian.net",
                    "routing_scope": "mission.sdlc",
                    "sdlc_profile": {
                        "repository": { "name": "service" },
                        "extensions": {
                            "release_train": "weekly",
                            "compliance": { "framework": "soc2" }
                        }
                    }
                }),
                secrets: json!({ "secret": "secret" }),
            },
            DEFAULT_BODY_LIMIT,
        );

        assert!(response.verified);
        assert_eq!(response.events[0].scope, "mission.sdlc");
        assert_eq!(
            response.events[0].correlation_key,
            "jira:acme.atlassian.net:issue:20"
        );
        assert_eq!(
            response.events[0]
                .payload
                .pointer("/profile/repository/name"),
            Some(&json!("service").into())
        );
        assert_eq!(
            response.events[0]
                .payload
                .pointer("/profile/extensions/compliance/framework"),
            Some(&json!("soc2").into())
        );
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

    #[test]
    fn builtins_enforce_the_configured_body_limit() {
        let body = br#"{"delivery":"d-1"}"#;
        let response = handle_generic(
            AdapterRequest {
                method: "POST".into(),
                headers: BTreeMap::new(),
                body_base64: base64::engine::general_purpose::STANDARD.encode(body),
                configuration: Value::Null,
                secrets: Value::Null,
            },
            body.len() - 1,
        );
        assert!(!response.verified);
        assert!(response.events.is_empty());
        assert!(
            response
                .errors
                .iter()
                .any(|error| error.contains("JSON body"))
        );
    }

    #[test]
    fn builtins_publish_typed_configuration_and_setup_guidance() {
        let generic = generic_metadata();
        let authentication = generic
            .fields
            .iter()
            .find(|field| field.name == "authentication")
            .unwrap();
        assert_eq!(
            authentication.value_type,
            RuninatorType::Enum(vec!["hmac_sha256".into(), "bearer".into()])
        );
        assert!(!generic.setup_instructions.is_empty());
        let jira = jira_metadata();
        assert!(!jira.setup_instructions.is_empty());
        assert_eq!(
            jira.polling_authentication,
            vec![AdapterAuthenticationKind::Secrets]
        );
        assert_eq!(jira.polling_secret_fields[0].name, "api_token");
        let github = github_metadata();
        assert!(!github.setup_instructions.is_empty());
        assert_eq!(
            github.polling_authentication,
            vec![
                AdapterAuthenticationKind::ExecutionProfile,
                AdapterAuthenticationKind::Secrets,
            ]
        );
        assert_eq!(github.polling_secret_fields[0].name, "access_token");
        assert_eq!(github.execution_profile_scopes, vec!["github"]);
    }
}

#[cfg(test)]
#[path = "poll_safety_tests.rs"]
mod poll_safety_tests;

mod host_limits;
use host_limits::HostLimits;

mod host_state;
use host_state::HostState;

mod invoke_request;
use invoke_request::InvokeRequest;

mod poll_invoke_request;
use poll_invoke_request::PollInvokeRequest;

mod validate_invoke_request;
use validate_invoke_request::ValidateInvokeRequest;
