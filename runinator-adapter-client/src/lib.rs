//! HTTP client facade for the adapter host. Both the web service (authoring, testing, webhook
//! delivery) and the engine's durable poll loop call the same process over the same contract, so
//! the url discovery, credential, and request shape live here once rather than in each caller.

use std::{sync::OnceLock, time::Duration};

use runinator_adapter_contract::{
    AdapterPollRequest, AdapterPollResponse, AdapterRequest, AdapterResponse,
};
use runinator_models::orchestration::AdapterKindCatalogEntry;
use serde::de::DeserializeOwned;
use thiserror::Error;
use tower::{ServiceExt, service_fn};
use tower_resilience_circuitbreaker::{CircuitBreakerError, CircuitBreakerLayer, FnClassifier};

/// The adapter host runs a dynamically loaded adapter in a disposable child process, so a call can
/// legitimately outlast an ordinary control-plane request. Verification is interactive and stays
/// short; a poll enumerates an upstream provider and is given the longer budget.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);
pub const POLL_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_CIRCUIT_FAILURE_THRESHOLD: usize = 5;
const DEFAULT_CIRCUIT_COOLDOWN_SECONDS: u64 = 30;

/// Errors from the adapter-host transport. A circuit-open result is distinct from a remote `503`:
/// no request was sent, so engine callers can use the advertised cooldown when rescheduling work.
#[derive(Debug, Error)]
pub enum AdapterClientError {
    #[error("adapter host configuration error: {0}")]
    Configuration(String),
    #[error("adapter host request error: {0}")]
    Request(#[source] reqwest::Error),
    #[error("adapter host returned {status}: {message}")]
    Http {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error("adapter host returned malformed output: {0}")]
    Decode(#[source] reqwest::Error),
    #[error("adapter-host circuit is open; retry after {retry_after_seconds}s")]
    CircuitOpen { retry_after_seconds: u64 },
}

pub type Result<T> = std::result::Result<T, AdapterClientError>;

type HttpResult = std::result::Result<reqwest::Response, reqwest::Error>;
type HttpClassifier = fn(&HttpResult) -> bool;
type HttpCircuitLayer = CircuitBreakerLayer<FnClassifier<HttpClassifier>>;

#[derive(Clone)]
struct AdapterCircuit {
    enabled: bool,
    cooldown: Duration,
    layer: HttpCircuitLayer,
}

impl AdapterCircuit {
    fn from_env() -> Self {
        let enabled = env_bool("RUNINATOR_ADAPTER_CLIENT_CIRCUIT_BREAKER_ENABLED", true);
        let failures = env_usize(
            "RUNINATOR_ADAPTER_CLIENT_CIRCUIT_BREAKER_FAILURE_THRESHOLD",
            DEFAULT_CIRCUIT_FAILURE_THRESHOLD,
        );
        let cooldown = Duration::from_secs(env_u64(
            "RUNINATOR_ADAPTER_CLIENT_CIRCUIT_BREAKER_COOLDOWN_SECONDS",
            DEFAULT_CIRCUIT_COOLDOWN_SECONDS,
        ));
        Self::new(enabled, failures, cooldown)
    }

    fn new(enabled: bool, failures: usize, cooldown: Duration) -> Self {
        let (layer, _) = CircuitBreakerLayer::builder()
            .name("adapter_host")
            .consecutive_failures(failures)
            .wait_duration_in_open(cooldown)
            .permitted_calls_in_half_open(1)
            .failure_classifier(adapter_host_failure as HttpClassifier)
            .on_state_transition(|from, to| {
                log::warn!("adapter-host circuit transitioned from {from:?} to {to:?}");
                metrics::counter!(
                    "runinator_adapter_host_circuit_breaker_transitions_total",
                    "target" => "adapter_host",
                    "from" => format!("{from:?}"),
                    "to" => format!("{to:?}"),
                )
                .increment(1);
            })
            .on_call_rejected(|| {
                log::warn!("adapter-host request rejected because the local circuit is open");
                metrics::counter!(
                    "runinator_adapter_host_circuit_breaker_rejections_total",
                    "target" => "adapter_host",
                )
                .increment(1);
            })
            .build_with_handle();
        Self {
            enabled,
            cooldown,
            layer,
        }
    }
}

fn adapter_host_failure(result: &HttpResult) -> bool {
    match result {
        Ok(response) => {
            response.status() == reqwest::StatusCode::REQUEST_TIMEOUT
                || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                || response.status().is_server_error()
        }
        Err(_) => true,
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// The configured adapter-host base url, for diagnostics that report what this process will call.
pub fn host_url() -> String {
    std::env::var("RUNINATOR_ADAPTER_HOST_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8790".into())
        .trim_end_matches('/')
        .to_owned()
}

/// The configured adapter-host credential. Exposed so a health endpoint can report whether one is
/// present without ever rendering its value.
pub fn host_token() -> Result<String> {
    std::env::var("RUNINATOR_ADAPTER_HOST_TOKEN").map_err(|_| {
        AdapterClientError::Configuration("RUNINATOR_ADAPTER_HOST_TOKEN is not configured".into())
    })
}

/// One pooled client for the whole process. A per-call `Client` would rebuild the connection pool
/// and tls configuration on every poll, which the poll loop does continuously.
fn client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<std::result::Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(POLL_TIMEOUT)
                .build()
                .map_err(|error| format!("adapter host client could not be created: {error}"))
        })
        .as_ref()
        .map_err(|error| AdapterClientError::Configuration(error.clone()))
}

fn circuit() -> &'static AdapterCircuit {
    static CIRCUIT: OnceLock<AdapterCircuit> = OnceLock::new();
    CIRCUIT.get_or_init(AdapterCircuit::from_env)
}

async fn send(builder: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    send_with_circuit(circuit(), builder).await
}

async fn send_with_circuit(
    circuit: &AdapterCircuit,
    builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response> {
    let request = builder.build().map_err(AdapterClientError::Request)?;
    let client = client()?.clone();
    if !circuit.enabled {
        return client
            .execute(request)
            .await
            .map_err(AdapterClientError::Request);
    }
    let service = service_fn(move |request| {
        let client = client.clone();
        async move { client.execute(request).await }
    });
    match circuit.layer.layer_fn(service).oneshot(request).await {
        Ok(response) => Ok(response),
        Err(CircuitBreakerError::OpenCircuit) => Err(AdapterClientError::CircuitOpen {
            retry_after_seconds: circuit.cooldown.as_secs().max(1),
        }),
        Err(CircuitBreakerError::Inner(error)) => Err(AdapterClientError::Request(error)),
    }
}

async fn get_json<T: DeserializeOwned>(path: &str) -> Result<T> {
    let response = send(
        client()?
            .get(format!("{}{path}", host_url()))
            .bearer_auth(host_token()?)
            .timeout(VERIFY_TIMEOUT),
    )
    .await?;
    decode(response).await
}

async fn post_json<T: DeserializeOwned>(
    path: &str,
    body: serde_json::Value,
    timeout: Duration,
) -> Result<T> {
    let response = send(
        client()?
            .post(format!("{}{path}", host_url()))
            .bearer_auth(host_token()?)
            .timeout(timeout)
            .json(&body),
    )
    .await?;
    decode(response).await
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(AdapterClientError::Http { status, message });
    }
    response.json().await.map_err(AdapterClientError::Decode)
}

/// The loaded adapter kinds and their health, as reported by the host's own catalog.
pub async fn kinds() -> Result<Vec<AdapterKindCatalogEntry>> {
    get_json("/kinds").await
}

/// The host's liveness and per-library load diagnostics.
pub async fn health() -> Result<serde_json::Value> {
    get_json("/health").await
}

/// Ask the host to rescan its adapter directory.
pub async fn reload() -> Result<serde_json::Value> {
    post_json("/reload", serde_json::json!({}), VERIFY_TIMEOUT).await
}

/// Verify a signed delivery and normalize it into canonical events.
pub async fn verify_normalize(kind: &str, request: AdapterRequest) -> Result<AdapterResponse> {
    post_json(
        "/verify-normalize",
        serde_json::json!({ "kind": kind, "request": request }),
        VERIFY_TIMEOUT,
    )
    .await
}

/// Pull the next batch of events for a polling adapter.
pub async fn poll(kind: &str, request: AdapterPollRequest) -> Result<AdapterPollResponse> {
    post_json(
        "/poll",
        serde_json::json!({ "kind": kind, "request": request }),
        POLL_TIMEOUT,
    )
    .await
}

#[cfg(test)]
mod resilience_tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
    };

    use super::*;

    fn status_server(statuses: Vec<u16>) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let server_calls = calls.clone();
        let task = thread::spawn(move || {
            for status in statuses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                server_calls.fetch_add(1, Ordering::SeqCst);
                let reason = if status == 200 { "OK" } else { "Test Failure" };
                let body = if status == 200 { "{}" } else { "failure" };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        (format!("http://{address}"), calls, task)
    }

    #[test]
    fn transient_failures_fast_fail_then_a_successful_probe_closes_the_adapter_circuit() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (base_url, calls, server) = status_server(vec![500, 500, 200]);
            let circuit = AdapterCircuit::new(true, 2, Duration::from_millis(1));
            let http = reqwest::Client::new();
            for _ in 0..2 {
                let response = send_with_circuit(&circuit, http.get(format!("{base_url}/failing")))
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    reqwest::StatusCode::INTERNAL_SERVER_ERROR
                );
            }
            let error = send_with_circuit(&circuit, http.get(format!("{base_url}/skipped")))
                .await
                .unwrap_err();
            assert!(matches!(
                error,
                AdapterClientError::CircuitOpen {
                    retry_after_seconds: 1
                }
            ));
            assert_eq!(calls.load(Ordering::SeqCst), 2);

            tokio::time::sleep(Duration::from_millis(5)).await;
            let response = send_with_circuit(&circuit, http.get(format!("{base_url}/probe")))
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            server.join().unwrap();
        });
    }

    #[test]
    fn normal_4xx_responses_do_not_open_the_adapter_circuit() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (base_url, calls, server) = status_server(vec![400, 400, 400]);
            let circuit = AdapterCircuit::new(true, 2, Duration::from_secs(1));
            let http = reqwest::Client::new();
            for _ in 0..3 {
                let response =
                    send_with_circuit(&circuit, http.get(format!("{base_url}/client-error")))
                        .await
                        .unwrap();
                assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
            }
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            server.join().unwrap();
        });
    }
}
