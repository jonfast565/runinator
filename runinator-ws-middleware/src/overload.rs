//! global overload protection for the http api: a hard cap on concurrently in-flight requests plus a
//! per-request timeout. the concurrency limit sheds excess load with `503` (via load-shed) instead of
//! queueing it without bound, and the timeout aborts a slow or stuck handler with `408`. both are
//! process-local, so each replica protects itself independently — the intended behavior for a
//! horizontally scaled api, and the natural complement to the per-principal rate limiter.

use std::time::Duration;

use axum::{
    BoxError, Router,
    error_handling::HandleErrorLayer,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tower::{ServiceBuilder, limit::GlobalConcurrencyLimitLayer, load_shed::LoadShedLayer};
use tower_http::timeout::TimeoutLayer;

#[cfg(test)]
#[path = "overload_tests.rs"]
mod tests;

/// runtime configuration for the overload-protection layers.
#[derive(Debug, Clone, Copy)]
pub struct OverloadConfig {
    pub enabled: bool,
    /// maximum requests processed concurrently before excess load is shed with `503`.
    pub max_concurrent_requests: usize,
    /// per-request wall-clock budget; a handler exceeding it is aborted with `408`.
    pub request_timeout: Duration,
}

impl Default for OverloadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_requests: 512,
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// wrap `router` with the concurrency cap and request timeout when enabled; a no-op passthrough
/// otherwise. layered so the shed `503`s and timeout `408`s it produces are still panic-caught and
/// access-logged by the outer catch-panic/trace layers applied after this call.
pub fn apply_overload_protection(router: Router, config: OverloadConfig) -> Router {
    if !config.enabled {
        return router;
    }
    // order is outer-to-inner: handle_error catches the shed error and turns it into a response so the
    // router stays infallible; load-shed converts concurrency-limit backpressure into an immediate
    // `Overloaded` instead of an unbounded wait; the concurrency limit gates in-flight work; the
    // timeout (innermost) bounds the wrapped handler and returns its own `408` response.
    let overload = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(handle_overload_error))
        .layer(LoadShedLayer::new())
        .layer(GlobalConcurrencyLimitLayer::new(
            config.max_concurrent_requests,
        ))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            config.request_timeout,
        ));
    router.layer(overload)
}

/// map a middleware error into a client response. a load-shed `Overloaded` is a retryable overload,
/// so it carries `Retry-After`; anything else is an unexpected internal middleware failure.
async fn handle_overload_error(error: BoxError) -> Response {
    if error.is::<tower::load_shed::error::Overloaded>() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [("Retry-After", "1")],
            "server overloaded",
        )
            .into_response();
    }
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal middleware error",
    )
        .into_response()
}
