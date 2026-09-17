//! Per-replica circuit breakers for the inbound HTTP API.
//!
//! The state machine comes from `tower-resilience-circuitbreaker`; this module only selects the
//! appropriate pre-built breaker for a request family and converts an open-circuit error into the
//! public HTTP response contract.

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tower::{ServiceExt, service_fn};
use tower_resilience_circuitbreaker::{CircuitBreakerError, CircuitBreakerLayer, FnClassifier};

/// Low-cardinality request families. The families deliberately keep unrelated route failure
/// histories apart while avoiding a circuit per unbounded URI parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitFamily {
    ReadQuery,
    WriteControl,
    ExternalIngress,
}

impl CircuitFamily {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadQuery => "read_query",
            Self::WriteControl => "write_control",
            Self::ExternalIngress => "external_ingress",
        }
    }
}

type HttpResult = Result<Response, Infallible>;
type HttpClassifier = fn(&HttpResult) -> bool;
type HttpCircuitLayer = CircuitBreakerLayer<FnClassifier<HttpClassifier>>;

fn make_breaker(config: CircuitBreakerConfig, family: CircuitFamily) -> HttpCircuitLayer {
    let label = family.label();
    let (layer, _) = CircuitBreakerLayer::builder()
        .name(label)
        .failure_rate_threshold(config.failure_rate_threshold)
        .minimum_number_of_calls(config.minimum_number_of_calls)
        .sliding_window_size(config.sliding_window_size)
        .wait_duration_in_open(config.cooldown)
        .permitted_calls_in_half_open(config.permitted_calls_in_half_open)
        .failure_classifier(inbound_failure as HttpClassifier)
        .on_state_transition(move |from, to| {
            log::warn!("HTTP circuit breaker '{label}' transitioned from {from:?} to {to:?}");
            metrics::counter!(
                "runinator_ws_circuit_breaker_transitions_total",
                "family" => label,
                "from" => format!("{from:?}"),
                "to" => format!("{to:?}"),
            )
            .increment(1);
        })
        .on_call_rejected(move || {
            metrics::counter!(
                "runinator_ws_circuit_breaker_rejections_total",
                "family" => label,
            )
            .increment(1);
        })
        // `build_with_handle` is essential because a fresh service is wrapped for each Axum
        // request below; the returned layer keeps one shared state machine per family.
        .build_with_handle()
        .expect("inbound circuit breaker configuration was validated at startup");
    layer
}

fn inbound_failure(result: &HttpResult) -> bool {
    match result {
        Ok(response) => {
            response.status() == StatusCode::REQUEST_TIMEOUT || response.status().is_server_error()
        }
        Err(never) => match *never {},
    }
}

fn is_bypassed(request: &Request<Body>) -> bool {
    let path = request.uri().path();
    matches!(path, "/health" | "/ready" | "/metrics")
        || path.starts_with("/auth")
        || path.starts_with("/openapi")
        || path == "/docs"
        || request.method() == Method::OPTIONS
        || request.headers().contains_key(header::UPGRADE)
}

fn is_external_ingress(request: &Request<Body>) -> bool {
    if request.method() != Method::POST {
        return false;
    }
    let path = request.uri().path();
    path.starts_with("/webhooks/orchestration/")
        || (path.starts_with("/workflows/") && path.ends_with("/ingress"))
        || (path.starts_with("/pipelines/") && path.ends_with("/ingress"))
}

/// Select and execute the request-family breaker. All actual state transitions and half-open
/// admission belong to the library layer; this adapter only maps an open circuit into HTTP.
pub async fn circuit_breaker_middleware(
    State(breakers): State<Arc<CircuitBreakers>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let Some((family, layer)) = breakers.select(&request) else {
        return next.run(request).await;
    };
    let service = service_fn(move |request| {
        let next = next.clone();
        async move { Ok::<Response, Infallible>(next.run(request).await) }
    });
    match layer.layer_fn(service).oneshot(request).await {
        Ok(response) => response,
        Err(CircuitBreakerError::OpenCircuit) => circuit_open_response(family, breakers.cooldown),
        Err(CircuitBreakerError::Inner(never)) => match never {},
    }
}

fn circuit_open_response(family: CircuitFamily, cooldown: Duration) -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::RETRY_AFTER, cooldown.as_secs().max(1).to_string())],
        "request family temporarily unavailable",
    )
        .into_response();
    response
        .extensions_mut()
        .insert(CircuitBreakerRejection { family });
    response
}

#[cfg(test)]
#[path = "circuit_breaker_tests.rs"]
mod tests;

mod circuit_breaker_config;
pub use circuit_breaker_config::CircuitBreakerConfig;

mod circuit_breaker_rejection;
pub use circuit_breaker_rejection::CircuitBreakerRejection;

mod circuit_breakers;
pub use circuit_breakers::CircuitBreakers;
