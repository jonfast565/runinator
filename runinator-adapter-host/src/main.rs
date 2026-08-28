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
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use runinator_adapter_contract::{
    ADAPTER_ABI_VERSION, AdapterMetadataEnvelope, AdapterPollRequest, AdapterPollResponse,
    AdapterRequest, AdapterResponse, FileOperationFn, HANDLE_SYMBOL, MARKER_SYMBOL,
    METADATA_SYMBOL, MarkerFn, NAME_SYMBOL, NameFn, POLL_SYMBOL, verify_bearer, verify_hmac_sha256,
};
use runinator_models::{
    orchestration::{
        AdapterConfigurationField, AdapterKindCatalogEntry, AdapterKindMetadata,
        NormalizedAdapterEvent,
    },
    types::RuninatorType,
};
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

#[derive(Clone, Copy)]
struct HostLimits {
    body_bytes: usize,
    output_bytes: usize,
    event_count: usize,
    timeout: Duration,
}

impl HostLimits {
    fn from_env() -> Self {
        Self {
            body_bytes: positive_env_usize("RUNINATOR_ADAPTER_BODY_LIMIT_BYTES")
                .unwrap_or(DEFAULT_BODY_LIMIT),
            output_bytes: positive_env_usize("RUNINATOR_ADAPTER_OUTPUT_LIMIT_BYTES")
                .unwrap_or(DEFAULT_OUTPUT_LIMIT),
            event_count: positive_env_usize("RUNINATOR_ADAPTER_EVENT_LIMIT")
                .unwrap_or(DEFAULT_EVENT_LIMIT),
            timeout: Duration::from_millis(
                positive_env_u64("RUNINATOR_ADAPTER_PLUGIN_TIMEOUT_MS")
                    .unwrap_or(DEFAULT_TIMEOUT.as_millis() as u64),
            ),
        }
    }
}

fn positive_env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
}

fn positive_env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
}

#[derive(Clone)]
struct HostState {
    token: Arc<String>,
    paths: Arc<Vec<PathBuf>>,
    limits: HostLimits,
    catalog: Arc<RwLock<BTreeMap<String, AdapterKindCatalogEntry>>>,
}

#[derive(Debug, Deserialize)]
struct InvokeRequest {
    kind: String,
    request: AdapterRequest,
}

#[derive(Debug, Deserialize)]
struct PollInvokeRequest {
    kind: String,
    request: AdapterPollRequest,
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
    if args.get(1).map(String::as_str) == Some("--child-poll") {
        return child_poll(
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
        limits: HostLimits::from_env(),
        catalog: Arc::new(RwLock::new(BTreeMap::new())),
    };
    reload_catalog(&state).await;
    let host_request_limit = state
        .limits
        .body_bytes
        .saturating_mul(2)
        .saturating_add(64 * 1024);
    let port = std::env::var("RUNINATOR_ADAPTER_HOST_PORT")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(8790);
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    let router = Router::new()
        .route("/live", get(live))
        .route("/health", get(health))
        .route("/kinds", get(kinds))
        .route("/reload", post(reload))
        .route("/verify-normalize", post(invoke))
        .route("/poll", post(poll))
        .layer(DefaultBodyLimit::max(host_request_limit))
        .with_state(state);
    axum::serve(listener, router).await?;
    Ok(())
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
            .unwrap_or_else(|error| AdapterResponse::rejected(error))
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
    let status = timeout(
        limits.timeout,
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
    if bytes.len() > limits.output_bytes {
        return Err("metadata output exceeds limit".into());
    }
    let envelope: AdapterMetadataEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if envelope.abi_version != ADAPTER_ABI_VERSION {
        return Err("adapter ABI version mismatch".into());
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
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
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
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
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

fn child_poll(
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
        let operation = library.get::<FileOperationFn>(POLL_SYMBOL)?;
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

fn builtin_catalog() -> BTreeMap<String, AdapterKindCatalogEntry> {
    [generic_metadata(), jira_metadata(), github_metadata()]
        .into_iter()
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
        event_names: vec![],
        canonical_pointers: vec![],
        capabilities: vec![],
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
        event_names: vec![],
        canonical_pointers: vec![
            "/delivery_id".into(),
            "/scope".into(),
            "/correlation_key".into(),
            "/event_type".into(),
        ],
        capabilities: vec!["hmac_sha256".into(), "bearer".into()],
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
        ],
        event_names: vec!["issue_updated".into(), "comment_created".into()],
        canonical_pointers: vec![
            "/issue/id".into(),
            "/issue/key".into(),
            "/changes".into(),
            "/provenance".into(),
        ],
        capabilities: vec!["bearer".into(), "polling".into()],
        setup_instructions: vec![
            "Choose webhook delivery or polling when creating the adapter.".into(),
            "For webhooks, configure Jira automation to POST issue and comment deliveries and send the selected Secret as a bearer token.".into(),
            "For polling, select an API-token Secret and configure the Jira base URL, account email, JQL, and cadence.".into(),
            "Set the stable Jira instance identity before enabling the adapter; it becomes part of every correlation scope.".into(),
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
        capabilities: vec!["hmac_sha256".into(), "polling".into()],
        setup_instructions: vec![
            "Choose webhook delivery or polling when creating the adapter.".into(),
            "For webhooks, use the displayed URL with application/json, select a matching webhook Secret, and subscribe to the required pull request, check run, and workflow run events.".into(),
            "For polling, select an access-token Secret, list repositories as owner/name, and configure the cadence.".into(),
        ],
    }
}

fn builtin_handle(kind: &str, request: AdapterRequest, body_limit: usize) -> AdapterResponse {
    match kind {
        "generic_webhook" => handle_generic(request, body_limit),
        "github" => handle_github(request, body_limit),
        "jira" => handle_jira(request, body_limit),
        _ => AdapterResponse::rejected("unknown built-in adapter"),
    }
}

/// Built-in polling deliberately produces the same normalized identities as webhook ingestion.
/// Checkpoints are high-water timestamps, not opaque page tokens, so a failed claim can replay an
/// overlap without losing updates; delivery IDs include the upstream update marker for dedupe.
async fn builtin_poll(kind: &str, request: AdapterPollRequest) -> AdapterPollResponse {
    match kind {
        "github" => poll_github(request).await,
        "jira" => poll_jira(request).await,
        _ => AdapterPollResponse {
            events: Vec::new(),
            checkpoint: request.checkpoint,
            retry_after_seconds: None,
            error: Some("adapter kind does not support polling".into()),
        },
    }
}

fn poll_secret<'a>(request: &'a AdapterPollRequest, name: &str) -> Result<&'a str, String> {
    configured_string(&request.secrets, name)
        .or_else(|_| configured_string(&request.configuration, name))
}

/// Read one stream's high-water mark. Checkpoints are per stream rather than per adapter: pull
/// requests, workflow runs, and check runs advance independently, and a single shared mark would
/// let a busy stream drag the watermark past events a quiet one had not emitted yet. The legacy
/// flat `{"updated_at": ...}` form seeds every stream so an in-flight adapter keeps its position.
fn stream_checkpoint(checkpoint: &Value, stream: &str) -> Option<String> {
    checkpoint
        .get("streams")
        .and_then(|streams| streams.get(stream))
        .and_then(Value::as_str)
        .or_else(|| checkpoint.get("updated_at").and_then(Value::as_str))
        .map(str::to_owned)
}

fn stream_checkpoints(marks: BTreeMap<String, String>) -> Value {
    json!({ "streams": marks })
}

/// Seed the next checkpoint with every mark the current one carries. A stream that reports nothing
/// new this pass must keep its position; rebuilding the map from only the streams that produced
/// events would reset the quiet ones to a cold start on the very next poll.
fn existing_streams(checkpoint: &Value) -> BTreeMap<String, String> {
    checkpoint
        .get("streams")
        .and_then(Value::as_object)
        .map(|streams| {
            streams
                .iter()
                .filter_map(|(name, value)| {
                    value.as_str().map(|value| (name.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn canonical_poll_timestamp(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .or_else(|_| chrono::DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .map(|value| value.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|_| value.to_owned())
}

fn poll_response(events: Vec<NormalizedAdapterEvent>, checkpoint: Value) -> AdapterPollResponse {
    AdapterPollResponse {
        events,
        checkpoint,
        retry_after_seconds: None,
        error: None,
    }
}

/// A poll that ran out of upstream quota. This is not an adapter fault: the checkpoint must be
/// preserved verbatim and the caller told when to come back, which is why it is distinct from an
/// ordinary error string.
struct RateLimited {
    retry_after_seconds: Option<u64>,
}

enum PollError {
    RateLimited(RateLimited),
    Failed(String),
}

impl From<String> for PollError {
    fn from(value: String) -> Self {
        Self::Failed(value)
    }
}

fn retry_after_header(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

/// The `Link: <url>; rel="next"` cursor GitHub returns on every paginated collection. Kept pure so
/// the header grammar is assertable without standing up a response.
fn parse_next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.contains("rel=\"next\"").then(|| {
            url.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_owned()
        })
    })
}

fn github_next_link(response: &reqwest::Response) -> Option<String> {
    parse_next_link(response.headers().get("link")?.to_str().ok()?)
}

/// GitHub reports exhausted quota two ways: 429, and 403 with a zeroed remaining counter. Treating
/// only the former as a rate limit turns the common case into a generic failure with the wrong
/// retry delay, so both are recognized here rather than at each call site.
fn github_rate_limited(response: &reqwest::Response) -> Option<RateLimited> {
    let exhausted = response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        || (response.status() == reqwest::StatusCode::FORBIDDEN
            && response
                .headers()
                .get("x-ratelimit-remaining")
                .and_then(|value| value.to_str().ok())
                == Some("0"));
    exhausted.then(|| RateLimited {
        retry_after_seconds: retry_after_header(response),
    })
}

async fn github_get(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<(Value, Option<String>), PollError> {
    let response = client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "runinator-adapter-host")
        .send()
        .await
        .map_err(|error| PollError::Failed(error.to_string()))?;
    if let Some(limited) = github_rate_limited(&response) {
        return Err(PollError::RateLimited(limited));
    }
    let next = github_next_link(&response);
    let response = response
        .error_for_status()
        .map_err(|error| PollError::Failed(error.to_string()))?;
    let body = response
        .json::<Value>()
        .await
        .map_err(|error| PollError::Failed(error.to_string()))?;
    Ok((body, next))
}

/// Walk a GitHub collection newest-first, stopping at the first page whose items are all older
/// than `since`. Without this a repository with more than one page of activity silently dropped
/// everything past the first hundred items the moment the watermark moved past them.
async fn github_collect(
    client: &reqwest::Client,
    token: &str,
    first_url: String,
    array_key: Option<&str>,
    since: Option<&str>,
    timestamp_of: impl Fn(&Value) -> String,
) -> Result<Vec<Value>, PollError> {
    let mut url = Some(first_url);
    let mut collected = Vec::new();
    for _ in 0..GITHUB_MAX_PAGES {
        let Some(current) = url.take() else { break };
        let (body, next) = github_get(client, token, &current).await?;
        let values = array_key
            .and_then(|key| body.get(key))
            .and_then(Value::as_array)
            .or_else(|| body.as_array())
            .cloned()
            .unwrap_or_default();
        if values.is_empty() {
            break;
        }
        let exhausted = since.is_some_and(|since| {
            values.iter().all(|value| {
                let stamp = timestamp_of(value);
                !stamp.is_empty() && stamp.as_str() < since
            })
        });
        collected.extend(values);
        if exhausted {
            break;
        }
        url = next;
    }
    Ok(collected)
}

fn github_repository_id(repository: &str, value: &Value) -> Result<String, String> {
    value
        .get("id")
        .map(value_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("GitHub repository '{repository}' has no stable id"))
}

fn github_poll_correlation(event_type: &str, id: &str, value: &Value) -> String {
    if event_type == "pull_request" {
        return format!("pr:{id}");
    }
    value
        .pointer("/pull_requests/0/id")
        .map(|value| format!("pr:{}", value_string(value)))
        .unwrap_or_else(|| format!("workflow:{id}"))
}

/// Advance a stream's mark only for an event that was actually emitted. Advancing before the
/// caller decides to skip a malformed item would carry the watermark past events that were never
/// reported, and they would never be enumerated again.
fn advance(marks: &mut BTreeMap<String, String>, stream: &str, stamp: &str) {
    if stamp.is_empty() {
        return;
    }
    let entry = marks.entry(stream.to_owned()).or_default();
    if stamp > entry.as_str() {
        *entry = stamp.to_owned();
    }
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
    let fallback_checkpoint = request.checkpoint.clone();
    match poll_github_inner(&request).await {
        Ok(response) => response,
        Err(PollError::RateLimited(limited)) => AdapterPollResponse {
            events: Vec::new(),
            checkpoint: fallback_checkpoint,
            retry_after_seconds: limited.retry_after_seconds,
            error: Some("GitHub rate limit reached".into()),
        },
        Err(PollError::Failed(error)) => AdapterPollResponse {
            events: Vec::new(),
            checkpoint: fallback_checkpoint,
            retry_after_seconds: None,
            error: Some(error),
        },
    }
}

async fn poll_github_inner(request: &AdapterPollRequest) -> Result<AdapterPollResponse, PollError> {
    let token = poll_secret(request, "access_token")?;
    let repositories = request
        .configuration
        .get("repositories")
        .and_then(Value::as_array)
        .ok_or_else(|| "GitHub polling requires configuration.repositories".to_string())?;
    let client = reqwest::Client::new();
    let mut marks = existing_streams(&request.checkpoint);
    let mut events = Vec::new();

    for repository in repositories.iter().filter_map(Value::as_str) {
        let (repository_info, _) = github_get(
            &client,
            token,
            &format!("https://api.github.com/repos/{repository}"),
        )
        .await?;
        let repository_id = github_repository_id(repository, &repository_info)?;

        for (url, event_type, array_key) in [
            (
                format!(
                    "https://api.github.com/repos/{repository}/pulls?state=all&sort=updated&direction=desc&per_page=100"
                ),
                "pull_request",
                None,
            ),
            (
                format!("https://api.github.com/repos/{repository}/actions/runs?per_page=100"),
                "workflow_run",
                Some("workflow_runs"),
            ),
        ] {
            let stream = format!("{repository_id}:{event_type}");
            let since = stream_checkpoint(&request.checkpoint, &stream);
            for value in github_collect(
                &client,
                token,
                url,
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
                let mut payload = value.clone();
                if let Some(object) = payload.as_object_mut() {
                    object.insert("repository".into(), repository_info.clone());
                    object.insert(event_type.into(), value.clone());
                }
                advance(&mut marks, &stream, &updated);
                events.push(NormalizedAdapterEvent {
                    source: "github".into(),
                    delivery_id: format!("github:{repository_id}:{event_type}:{id}:{updated}"),
                    event_type: event_type.into(),
                    scope: format!("github:repository:{repository_id}"),
                    correlation_key: github_poll_correlation(event_type, &id, &value),
                    subject_revision: value
                        .get("head_sha")
                        .or_else(|| value.pointer("/head/sha"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    occurred_at: parse_occurred_at(&Value::String(updated)).ok(),
                    payload: payload.into(),
                    provenance: Value::Null.into(),
                });
            }
        }

        let stream = format!("{repository_id}:check_run");
        let since = stream_checkpoint(&request.checkpoint, &stream);
        let mut commits_url =
            format!("https://api.github.com/repos/{repository}/commits?per_page=100");
        if let Some(since) = &since {
            commits_url.push_str("&since=");
            commits_url.push_str(&urlencoding::encode(since));
        }
        let (commits, _) = github_get(&client, token, &commits_url).await?;
        // one check-runs request per commit is the expensive part of this poll. bounding it keeps a
        // busy repository from spending the hourly quota in a single pass; `since` above is what
        // keeps the steady-state list short in the first place.
        for commit in commits
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(GITHUB_CHECK_RUN_COMMIT_BUDGET)
        {
            let Some(sha) = commit.get("sha").and_then(Value::as_str) else {
                continue;
            };
            let checks = github_collect(
                &client,
                token,
                format!(
                    "https://api.github.com/repos/{repository}/commits/{sha}/check-runs?per_page=100"
                ),
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
                advance(&mut marks, &stream, &updated);
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
                    payload: json!({ "repository": repository_info, "check_run": check }).into(),
                    provenance: Value::Null.into(),
                });
            }
        }
    }

    if request.initialize {
        // a first poll establishes the high-water mark without replaying history, so every stream
        // that reported anything is stamped at its newest item and no event is emitted.
        events.clear();
    }
    Ok(poll_response(events, stream_checkpoints(marks)))
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
    let fallback_checkpoint = request.checkpoint.clone();
    match poll_jira_inner(&request).await {
        Ok(response) => response,
        Err(PollError::RateLimited(limited)) => AdapterPollResponse {
            events: Vec::new(),
            checkpoint: fallback_checkpoint,
            retry_after_seconds: limited.retry_after_seconds,
            error: Some("Jira rate limit reached".into()),
        },
        Err(PollError::Failed(error)) => AdapterPollResponse {
            events: Vec::new(),
            checkpoint: fallback_checkpoint,
            retry_after_seconds: None,
            error: Some(error),
        },
    }
}

async fn poll_jira_inner(request: &AdapterPollRequest) -> Result<AdapterPollResponse, PollError> {
    let base_url = configured_string(&request.configuration, "base_url")?.trim_end_matches('/');
    let email = configured_string(&request.configuration, "email")?;
    let token = poll_secret(request, "api_token")?;
    let instance_id = configured_string(&request.configuration, "instance_id")?;
    let jql = configured_string(&request.configuration, "jql")?;

    let issue_stream = format!("{instance_id}:issue");
    let comment_stream = format!("{instance_id}:comment");
    let previous = stream_checkpoint(&request.checkpoint, &issue_stream);
    let comment_previous = stream_checkpoint(&request.checkpoint, &comment_stream);
    let query = match previous.as_deref().and_then(jira_relative_bound) {
        Some(bound) => format!("({jql}) AND updated >= \"{bound}\""),
        None => jql.to_owned(),
    };

    let client = reqwest::Client::new();
    let mut issues = Vec::new();
    let mut next_page_token: Option<String> = None;
    for page in 0..JIRA_MAX_PAGES {
        let requested_page_token = next_page_token.clone();
        let mut call = client
            .get(format!("{base_url}/rest/api/3/search/jql"))
            .basic_auth(email, Some(token))
            .query(&[
                ("jql", query.as_str()),
                ("maxResults", "100"),
                ("fields", "summary,project,updated,comment"),
            ]);
        if let Some(token) = &next_page_token {
            call = call.query(&[("nextPageToken", token)]);
        }
        let response = call
            .send()
            .await
            .map_err(|error| PollError::Failed(error.to_string()))?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(PollError::RateLimited(RateLimited {
                retry_after_seconds: retry_after_header(&response),
            }));
        }
        let response = response
            .error_for_status()
            .map_err(|error| PollError::Failed(error.to_string()))?
            .json::<Value>()
            .await
            .map_err(|error| PollError::Failed(error.to_string()))?;
        issues.extend(
            response
                .get("issues")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        next_page_token = response
            .get("nextPageToken")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|value| !value.is_empty());
        if next_page_token.is_some() && next_page_token == requested_page_token {
            return Err(PollError::Failed(
                "Jira polling received a repeated page token".to_string(),
            ));
        }
        if next_page_token.is_none() {
            break;
        }
        if page + 1 == JIRA_MAX_PAGES {
            return Err(PollError::Failed(format!(
                "Jira polling exceeded {JIRA_MAX_PAGES} result pages"
            )));
        }
    }

    let mut marks = existing_streams(&request.checkpoint);
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
        advance(&mut marks, &issue_stream, &updated);
        events.push(NormalizedAdapterEvent {
            source: "jira".into(),
            delivery_id: format!("jira:{instance_id}:issue:{issue_id}:{updated}"),
            event_type: "issue_updated".into(),
            scope: format!("jira:{instance_id}:project:{project}"),
            correlation_key: format!("issue:{issue_id}"),
            subject_revision: None,
            occurred_at: parse_occurred_at(&Value::String(updated.clone())).ok(),
            payload: json!({ "issue": issue.clone() }).into(),
            provenance: Value::Null.into(),
        });
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
            advance(&mut marks, &comment_stream, &comment_updated);
            events.push(NormalizedAdapterEvent {
                source: "jira".into(),
                delivery_id: format!(
                    "jira:{instance_id}:comment:{}:{comment_updated}",
                    comment.get("id").map(value_string).unwrap_or_default()
                ),
                event_type: "comment_created".into(),
                scope: format!("jira:{instance_id}:project:{project}"),
                correlation_key: format!("issue:{issue_id}"),
                subject_revision: None,
                occurred_at: parse_occurred_at(&Value::String(comment_updated)).ok(),
                payload: json!({"issue": issue, "comment": comment}).into(),
                provenance: Value::Null.into(),
            });
        }
    }
    if request.initialize {
        events.clear();
    }
    Ok(poll_response(events, stream_checkpoints(marks)))
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
            return timestamp_to_datetime(timestamp);
        }
        return Err("occurrence time is neither RFC3339 nor an epoch timestamp".into());
    }
    value
        .as_i64()
        .ok_or_else(|| "occurrence time must be a string or integer".into())
        .and_then(timestamp_to_datetime)
}

fn timestamp_to_datetime(timestamp: i64) -> Result<chrono::DateTime<chrono::Utc>, String> {
    use chrono::{TimeZone, Utc};

    let (seconds, nanos) = if timestamp.abs() >= 10_000_000_000 {
        (
            timestamp.div_euclid(1_000),
            timestamp.rem_euclid(1_000) as u32 * 1_000_000,
        )
    } else {
        (timestamp, 0)
    };
    Utc.timestamp_opt(seconds, nanos)
        .single()
        .ok_or_else(|| "occurrence time is outside the supported range".into())
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
    let provenance = extract_runinator_operation_key(&payload)
        .map(|operation_key| json!({ "operation_key": operation_key }))
        .unwrap_or(Value::Null);
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
                    "/check_run/completed_at",
                    "/check_run/started_at",
                    "/workflow_run/updated_at",
                ],
            ),
            payload: payload.into(),
            provenance: provenance.into(),
        }],
        errors: vec![],
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
    let provenance = extract_runinator_operation_key(&payload)
        .map(|operation_key| json!({ "operation_key": operation_key }))
        .unwrap_or(Value::Null);
    AdapterResponse {
        verified: true,
        events: vec![NormalizedAdapterEvent {
            source: "jira".into(),
            delivery_id,
            event_type,
            scope: format!("jira:{instance_id}:project:{project_id}"),
            correlation_key: format!("issue:{issue_id}"),
            subject_revision: None,
            occurred_at: first_occurred_at(&payload, &["/timestamp"]),
            payload: payload.into(),
            provenance: provenance.into(),
        }],
        errors: vec![],
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
    fn github_polling_uses_the_same_stable_scope_and_correlation_as_webhooks() {
        let repository = json!({ "id": 10, "full_name": "octo/example" });
        assert_eq!(
            github_repository_id("octo/example", &repository).unwrap(),
            "10"
        );
        assert_eq!(
            github_poll_correlation("pull_request", "20", &json!({ "id": 20 })),
            "pr:20"
        );
        assert_eq!(
            github_poll_correlation(
                "workflow_run",
                "30",
                &json!({ "pull_requests": [{ "id": 20 }] }),
            ),
            "pr:20"
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

    #[test]
    fn each_stream_keeps_its_own_high_water_mark() {
        // one shared mark let a busy stream drag the watermark past events a quiet one had not
        // emitted yet, silently dropping them. the marks must move independently.
        let mut marks = BTreeMap::new();
        advance(&mut marks, "7:pull_request", "2026-08-27T10:00:00+00:00");
        advance(&mut marks, "7:workflow_run", "2026-08-27T12:00:00+00:00");
        advance(&mut marks, "7:pull_request", "2026-08-27T09:00:00+00:00");

        let checkpoint = stream_checkpoints(marks);
        assert_eq!(
            stream_checkpoint(&checkpoint, "7:pull_request").as_deref(),
            Some("2026-08-27T10:00:00+00:00"),
            "a later mark on another stream must not drag this one forward, and a mark never moves back"
        );
        assert_eq!(
            stream_checkpoint(&checkpoint, "7:workflow_run").as_deref(),
            Some("2026-08-27T12:00:00+00:00")
        );
        assert_eq!(stream_checkpoint(&checkpoint, "7:check_run"), None);
    }

    #[test]
    fn a_quiet_stream_keeps_its_position_across_a_poll() {
        // rebuilding the map from only the streams that produced events would reset every quiet
        // stream to a cold start, replaying its whole history on the next poll.
        let checkpoint = json!({ "streams": { "7:check_run": "2026-08-27T08:00:00+00:00" } });
        let mut marks = existing_streams(&checkpoint.clone().into());
        advance(&mut marks, "7:pull_request", "2026-08-27T10:00:00+00:00");

        let next = stream_checkpoints(marks);
        assert_eq!(
            stream_checkpoint(&next, "7:check_run").as_deref(),
            Some("2026-08-27T08:00:00+00:00")
        );
    }

    #[test]
    fn a_legacy_flat_checkpoint_seeds_every_stream() {
        // adapters already in flight carry the old single-mark shape; it must keep its position
        // rather than reading as a cold start.
        let legacy: Value = json!({ "updated_at": "2026-08-27T08:00:00+00:00" }).into();
        assert_eq!(
            stream_checkpoint(&legacy, "7:pull_request").as_deref(),
            Some("2026-08-27T08:00:00+00:00")
        );
        assert_eq!(
            stream_checkpoint(&legacy, "7:check_run").as_deref(),
            Some("2026-08-27T08:00:00+00:00")
        );
    }

    #[test]
    fn a_next_page_cursor_is_read_from_the_link_header() {
        // without following this cursor a repository with more than one page of activity dropped
        // everything past the first hundred items as soon as the watermark moved past them.
        assert_eq!(
            parse_next_link(
                "<https://api.github.com/repos/o/r/pulls?page=2>; rel=\"next\", <https://api.github.com/repos/o/r/pulls?page=9>; rel=\"last\""
            )
            .as_deref(),
            Some("https://api.github.com/repos/o/r/pulls?page=2")
        );
        assert_eq!(
            parse_next_link("<https://api.github.com/repos/o/r/pulls?page=9>; rel=\"last\""),
            None,
            "the last page has no next cursor and must end the walk"
        );
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
        assert!(!jira_metadata().setup_instructions.is_empty());
        assert!(!github_metadata().setup_instructions.is_empty());
    }
}
