//! HTTP client facade for the adapter host. Both the web service (authoring, testing, webhook
//! delivery) and the engine's durable poll loop call the same process over the same contract, so
//! the url discovery, credential, and request shape live here once rather than in each caller.

use std::{sync::OnceLock, time::Duration};

use runinator_adapter_contract::{
    AdapterPollRequest, AdapterPollResponse, AdapterRequest, AdapterResponse,
};
use runinator_models::orchestration::AdapterKindCatalogEntry;
use serde::de::DeserializeOwned;

/// The adapter host runs a dynamically loaded adapter in a disposable child process, so a call can
/// legitimately outlast an ordinary control-plane request. Verification is interactive and stays
/// short; a poll enumerates an upstream provider and is given the longer budget.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);
pub const POLL_TIMEOUT: Duration = Duration::from_secs(120);

/// The configured adapter-host base url, for diagnostics that report what this process will call.
pub fn host_url() -> String {
    std::env::var("RUNINATOR_ADAPTER_HOST_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8790".into())
        .trim_end_matches('/')
        .to_owned()
}

/// The configured adapter-host credential. Exposed so a health endpoint can report whether one is
/// present without ever rendering its value.
pub fn host_token() -> Result<String, String> {
    std::env::var("RUNINATOR_ADAPTER_HOST_TOKEN")
        .map_err(|_| "RUNINATOR_ADAPTER_HOST_TOKEN is not configured".into())
}

/// One pooled client for the whole process. A per-call `Client` would rebuild the connection pool
/// and tls configuration on every poll, which the poll loop does continuously.
fn client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(POLL_TIMEOUT)
                .build()
                .map_err(|error| format!("adapter host client could not be created: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

async fn get_json<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let response = client()?
        .get(format!("{}{path}", host_url()))
        .bearer_auth(host_token()?)
        .timeout(VERIFY_TIMEOUT)
        .send()
        .await
        .map_err(|error| format!("adapter host is unavailable: {error}"))?;
    decode(response).await
}

async fn post_json<T: DeserializeOwned>(
    path: &str,
    body: serde_json::Value,
    timeout: Duration,
) -> Result<T, String> {
    let response = client()?
        .post(format!("{}{path}", host_url()))
        .bearer_auth(host_token()?)
        .timeout(timeout)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("adapter host is unavailable: {error}"))?;
    decode(response).await
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, String> {
    let status = response.status();
    if !status.is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(format!("adapter host returned {status}: {message}"));
    }
    response
        .json()
        .await
        .map_err(|error| format!("adapter host returned malformed output: {error}"))
}

/// The loaded adapter kinds and their health, as reported by the host's own catalog.
pub async fn kinds() -> Result<Vec<AdapterKindCatalogEntry>, String> {
    get_json("/kinds").await
}

/// The host's liveness and per-library load diagnostics.
pub async fn health() -> Result<serde_json::Value, String> {
    get_json("/health").await
}

/// Ask the host to rescan its adapter directory.
pub async fn reload() -> Result<serde_json::Value, String> {
    post_json("/reload", serde_json::json!({}), VERIFY_TIMEOUT).await
}

/// Verify a signed delivery and normalize it into canonical events.
pub async fn verify_normalize(
    kind: &str,
    request: AdapterRequest,
) -> Result<AdapterResponse, String> {
    post_json(
        "/verify-normalize",
        serde_json::json!({ "kind": kind, "request": request }),
        VERIFY_TIMEOUT,
    )
    .await
}

/// Pull the next batch of events for a polling adapter.
pub async fn poll(kind: &str, request: AdapterPollRequest) -> Result<AdapterPollResponse, String> {
    post_json(
        "/poll",
        serde_json::json!({ "kind": kind, "request": request }),
        POLL_TIMEOUT,
    )
    .await
}
