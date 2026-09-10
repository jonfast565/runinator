//! HTTP client facade for the adapter host. Both the web service (authoring, testing, webhook
//! delivery) and the engine's durable poll loop call the same process over the same contract, so
//! the url discovery, credential, and request shape live here once rather than in each caller.

use async_trait::async_trait;
use std::time::Duration;

mod client;
pub use client::{
    AdapterHostAdmin, AdapterHostClient, AdapterPoller, AdapterVerifier, HttpAdapterHostClient,
};

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

async fn send_with_circuit(
    client: &reqwest::Client,
    circuit: &AdapterCircuit,
    builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response> {
    let request = builder.build().map_err(AdapterClientError::Request)?;
    let client = client.clone();
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

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(AdapterClientError::Http { status, message });
    }
    response.json().await.map_err(AdapterClientError::Decode)
}

#[cfg(test)]
mod resilience_tests;
