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

/// Runtime policy for all inbound circuit families on one API replica.
#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerConfig {
    pub enabled: bool,
    /// Failure fraction that opens a closed circuit once its sample is eligible.
    pub failure_rate_threshold: f64,
    /// Minimum number of handler calls sampled before evaluating the failure rate.
    pub minimum_number_of_calls: usize,
    /// Number of calls retained by the count-based sliding window.
    pub sliding_window_size: usize,
    /// How long an open circuit rejects requests before a half-open probe is admitted.
    pub cooldown: Duration,
    /// Number of simultaneous recovery probes admitted while half-open.
    pub permitted_calls_in_half_open: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            failure_rate_threshold: 0.5,
            minimum_number_of_calls: 20,
            sliding_window_size: 100,
            cooldown: Duration::from_secs(30),
            permitted_calls_in_half_open: 1,
        }
    }
}

impl CircuitBreakerConfig {
    /// Check CLI/environment-derived values before the server begins accepting traffic.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.failure_rate_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.failure_rate_threshold)
        {
            return Err("circuit breaker failure-rate threshold must be between 0 and 1");
        }
        if self.minimum_number_of_calls == 0 {
            return Err("circuit breaker minimum calls must be greater than zero");
        }
        if self.sliding_window_size == 0 {
            return Err("circuit breaker window size must be greater than zero");
        }
        if self.minimum_number_of_calls > self.sliding_window_size {
            return Err("circuit breaker minimum calls cannot exceed the window size");
        }
        if self.cooldown.is_zero() {
            return Err("circuit breaker cooldown must be greater than zero");
        }
        if self.permitted_calls_in_half_open == 0 {
            return Err("circuit breaker half-open probe count must be greater than zero");
        }
        Ok(())
    }
}

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

/// Marks a response synthesized because an API circuit was open. The outer access-metrics layer
/// reads this extension so it never attributes the `503` to overload protection.
#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerRejection {
    pub family: CircuitFamily,
}

type HttpResult = Result<Response, Infallible>;
type HttpClassifier = fn(&HttpResult) -> bool;
type HttpCircuitLayer = CircuitBreakerLayer<FnClassifier<HttpClassifier>>;

/// Stateful selector containing one library-owned breaker per request family.
#[derive(Clone)]
pub struct CircuitBreakers {
    enabled: bool,
    cooldown: Duration,
    read_query: HttpCircuitLayer,
    write_control: HttpCircuitLayer,
    external_ingress: HttpCircuitLayer,
}

impl CircuitBreakers {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        config
            .validate()
            .expect("inbound circuit breaker configuration was validated at startup");
        Self {
            enabled: config.enabled,
            cooldown: config.cooldown,
            read_query: make_breaker(config, CircuitFamily::ReadQuery),
            write_control: make_breaker(config, CircuitFamily::WriteControl),
            external_ingress: make_breaker(config, CircuitFamily::ExternalIngress),
        }
    }

    fn select(&self, request: &Request<Body>) -> Option<(CircuitFamily, HttpCircuitLayer)> {
        if !self.enabled || is_bypassed(request) {
            return None;
        }
        let family = if is_external_ingress(request) {
            CircuitFamily::ExternalIngress
        } else if matches!(*request.method(), Method::GET | Method::HEAD) {
            CircuitFamily::ReadQuery
        } else {
            CircuitFamily::WriteControl
        };
        let layer = match family {
            CircuitFamily::ReadQuery => self.read_query.clone(),
            CircuitFamily::WriteControl => self.write_control.clone(),
            CircuitFamily::ExternalIngress => self.external_ingress.clone(),
        };
        Some((family, layer))
    }
}

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
        .build_with_handle();
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
