//! the http surface: assembles every domain's routes and wraps them in the middleware stack.
//!
//! route registration itself lives next to the handlers it serves — each `crate::handlers::<domain>`
//! module (plus `WS` and `openapi`) exposes a `routes()` fn returning its own `Router`, and
//! this file merges them. `Router::merge` panics on a duplicate method+path, so the split cannot
//! silently shadow a route; `openapi::route_parity` lints the merged set against the documented one.

use std::sync::Arc;

use axum::http::HeaderValue;
use axum::response::IntoResponse;
use axum::{Extension, Router, extract::DefaultBodyLimit, middleware::from_fn_with_state};
use runinator_blob::BlobStore;
use runinator_broker::Broker;
use runinator_engine::services::{
    AutomationOperations, CatalogOperations, ConsoleOperations, DebugOperations,
    ExecutionProfileOperations, FunctionInvocations, FunctionPackages, NotificationOperations,
    PackOperations, PipelineOperations, RunOperations, SchedulingOperations, WorkflowAuthoring,
    WorkflowFiles,
};
use runinator_provisioner::ProvisionerRegistry;
use runinator_store::DatabaseImpl;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use crate::auth::{AuthConfig, AuthState, auth_middleware};
use crate::events::EventSender;
use crate::handlers::{
    adapters, agents, artifacts, auth, authz, automation, billing, catalog, catalog_metadata,
    console, credentials, debug, execution_profiles, files, function_invocations, functions,
    health, ingress_control, notifications, observability, orchestrations, orgs, packs, pipelines,
    providers, provisioning, replicas, rexrap, runs, schedules, supervisor, triggers, workflow_vm,
    workflows,
};
use crate::models::{ApiError, ApiResponse};
use crate::overload::{OverloadConfig, apply_overload_protection};
use crate::rate_limit::{RateLimitConfig, RateLimiter, rate_limit_middleware};
use crate::{openapi, websocket};

/// Browser origins allowed to call the API directly by default.
///
/// The hosted Command Center uses a same-origin reverse proxy and therefore needs no CORS entry.
/// These entries cover the desktop shell and the documented local Vite development server without
/// exposing an auth-disabled local service to every web page the operator visits.
pub const DEFAULT_CORS_ALLOWED_ORIGINS: &str = "tauri://localhost,http://tauri.localhost,https://tauri.localhost,http://localhost:5173,http://127.0.0.1:5173";

#[derive(Debug, Clone)]
pub struct CorsConfig {
    allowed_origins: Vec<HeaderValue>,
}

impl CorsConfig {
    /// Parse exact origins supplied by the CLI/environment. A wildcard is deliberately rejected:
    /// when authentication is disabled, allowing every origin turns any visited website into an
    /// administrator of the operator's local Runinator service.
    pub fn new(origins: Vec<String>) -> Result<Self, String> {
        let mut allowed_origins = Vec::new();
        for raw in origins {
            let origin = raw.trim();
            if origin.is_empty() {
                continue;
            }
            if origin == "*" {
                return Err("CORS allowed origins must be explicit; '*' is not permitted".into());
            }
            let uri = origin
                .parse::<axum::http::Uri>()
                .map_err(|err| format!("invalid CORS origin '{origin}': {err}"))?;
            if uri.scheme().is_none()
                || uri.authority().is_none()
                || uri
                    .path_and_query()
                    .is_some_and(|path| path.as_str() != "/")
            {
                return Err(format!(
                    "invalid CORS origin '{origin}': expected scheme://host[:port] with no path"
                ));
            }
            let value = HeaderValue::from_str(origin)
                .map_err(|err| format!("invalid CORS origin '{origin}': {err}"))?;
            if !allowed_origins.contains(&value) {
                allowed_origins.push(value);
            }
        }
        Ok(Self { allowed_origins })
    }

    pub fn allowed_origin_count(&self) -> usize {
        self.allowed_origins.len()
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self::new(
            DEFAULT_CORS_ALLOWED_ORIGINS
                .split(',')
                .map(str::to_string)
                .collect(),
        )
        .expect("built-in CORS origins are valid")
    }
}

#[allow(clippy::too_many_arguments)] // router assembly keeps each injected runtime dependency explicit.
pub fn build_router<T: DatabaseImpl>(
    pool: Arc<T>,
    events: EventSender,
    broker: Arc<dyn Broker>,
    blobs: Arc<dyn BlobStore>,
    provisioner: Arc<ProvisionerRegistry>,
    auth: AuthConfig,
    cors: CorsConfig,
    rate_limit: RateLimitConfig,
    overload: OverloadConfig,
) -> Router {
    let auth_config_arc = Arc::new(auth);
    let rate_limiter = Arc::new(RateLimiter::new(rate_limit));
    let run_operations = Arc::new(RunOperations::new(
        pool.clone(),
        broker.clone(),
        events.publisher(),
        events.embedded_engine_signals(),
    ));
    let workflow_authoring = Arc::new(WorkflowAuthoring::new(pool.clone(), events.publisher()));
    let execution_profile_operations = Arc::new(ExecutionProfileOperations::new(pool.clone()));
    let pipeline_operations = Arc::new(PipelineOperations::new(
        pool.clone(),
        broker.clone(),
        events.publisher(),
        events.embedded_engine_signals(),
    ));
    let function_packages = Arc::new(FunctionPackages::new(pool.clone(), blobs.clone()));
    let workflow_files = Arc::new(WorkflowFiles::new(pool.clone(), blobs.clone()));
    let automation_operations = Arc::new(AutomationOperations::new(pool.clone()));
    let scheduling_operations = Arc::new(SchedulingOperations::new(
        pool.clone(),
        events.publisher(),
        events.embedded_engine_signals(),
    ));
    let notification_operations = Arc::new(NotificationOperations::new(
        pool.clone(),
        events.publisher(),
    ));
    let function_invocations = Arc::new(FunctionInvocations::new(
        pool.clone(),
        broker.clone(),
        events.publisher(),
        events.embedded_engine_signals(),
    ));
    let catalog_operations = Arc::new(CatalogOperations::new(pool.clone()));
    let debug_operations = Arc::new(DebugOperations::new(
        pool.clone(),
        events.publisher(),
        events.embedded_engine_signals(),
    ));
    let console_operations = Arc::new(ConsoleOperations::new(
        pool.clone(),
        broker.clone(),
        events.publisher(),
        events.embedded_engine_signals(),
    ));
    let pack_operations = Arc::new(PackOperations::new(
        pool.clone(),
        blobs.clone(),
        events.publisher(),
    ));
    let cors = cors_layer(&cors);

    let router = Router::new()
        .merge(health::routes(pool.clone()))
        .merge(openapi::routes())
        .merge(websocket::routes(pool.clone()))
        .merge(workflows::routes(pool.clone()))
        .merge(rexrap::routes(pool.clone()))
        .merge(packs::routes(pool.clone()))
        .merge(triggers::routes(pool.clone()))
        .merge(runs::routes(pool.clone()))
        .merge(pipelines::routes(pool.clone()))
        .merge(orchestrations::routes(pool.clone(), events.publisher()))
        .merge(adapters::routes(pool.clone(), events.publisher()))
        .merge(replicas::routes(pool.clone()))
        .merge(agents::routes(pool.clone()))
        .merge(provisioning::routes())
        .merge(artifacts::routes::<T>())
        .merge(files::routes::<T>())
        .merge(notifications::routes(pool.clone()))
        .merge(schedules::routes(pool.clone()))
        .merge(debug::routes(pool.clone()))
        .merge(supervisor::routes())
        .merge(workflow_vm::routes(pool.clone()))
        .merge(catalog::routes(pool.clone()))
        .merge(automation::routes(pool.clone()))
        .merge(observability::routes(pool.clone()))
        .merge(ingress_control::routes(pool.clone()))
        .merge(credentials::routes(pool.clone()))
        .merge(execution_profiles::routes::<T>())
        .merge(providers::routes(pool.clone()))
        .merge(functions::routes(pool.clone()))
        .merge(function_invocations::routes(pool.clone()))
        .merge(console::routes(pool.clone()))
        .merge(catalog_metadata::routes())
        .merge(auth::routes(pool.clone()))
        .merge(authz::routes(pool.clone()))
        .merge(orgs::routes(pool.clone()))
        .merge(billing::routes(pool.clone()))
        .layer(Extension(events))
        .layer(Extension(pack_operations))
        .layer(Extension(console_operations))
        .layer(Extension(debug_operations))
        .layer(Extension(catalog_operations))
        .layer(Extension(function_invocations))
        .layer(Extension(notification_operations))
        .layer(Extension(scheduling_operations))
        .layer(Extension(automation_operations))
        .layer(Extension(function_packages))
        .layer(Extension(workflow_files))
        .layer(Extension(pipeline_operations))
        .layer(Extension(run_operations))
        .layer(Extension(workflow_authoring))
        .layer(Extension(execution_profile_operations))
        .layer(Extension(broker))
        .layer(Extension(blobs))
        .layer(Extension(provisioner))
        .layer(Extension(auth_config_arc.clone()))
        // the rate limiter is layered inside the auth middleware so it can key by the resolved
        // principal; auth inserts the `AuthContext` before this layer runs.
        .layer(from_fn_with_state(rate_limiter, rate_limit_middleware))
        .layer(from_fn_with_state(
            AuthState {
                config: auth_config_arc,
                db: pool.clone(),
            },
            auth_middleware::<T>,
        ))
        .layer(cors)
        // cap request bodies for every route. layered here (after all routes are added) so axum
        // actually applies it; placed before `Router::new()` had any routes, it wrapped nothing and
        // requests silently fell back to axum's stricter 2 MB default. 10 MB accommodates pack uploads.
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024));

    // global overload protection (concurrency cap + per-request timeout) wraps everything above so a
    // flood is shed or timed out before auth/handler work runs. applied here rather than in the chain
    // so the catch-panic and trace layers below stay outermost and still cover its 503/408 responses.
    let router = apply_overload_protection(router, overload);

    router
        // recover from any panic in a handler or inner middleware so a single bad request returns a
        // 500 instead of dropping the connection or poisoning the runtime.
        .layer(CatchPanicLayer::custom(handle_panic))
        // Admission queues use 429 as backpressure. Always make the retry contract explicit,
        // including webhook adapters whose shared handler returns the historical JSON tuple.
        .layer(axum::middleware::from_fn(retry_after_backpressure))
        // outermost layer: open a request span parented to any inbound w3c trace context so logs and
        // otel spans for this request continue the caller's distributed trace.
        .layer(axum::middleware::from_fn(trace_propagation_middleware))
}

async fn retry_after_backpressure(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    if response.status() == axum::http::StatusCode::TOO_MANY_REQUESTS
        && !response
            .headers()
            .contains_key(axum::http::header::RETRY_AFTER)
    {
        response.headers_mut().insert(
            axum::http::header::RETRY_AFTER,
            axum::http::HeaderValue::from_static("5"),
        );
    }
    response
}

fn cors_layer(config: &CorsConfig) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(config.allowed_origins.clone()))
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any)
}

const REQUEST_ID_HEADER: &str = "x-request-id";

/// open a per-request tracing span, re-parent it onto any inbound `traceparent` header so the server
/// side of a distributed trace links to the caller, and log an access line with status/duration once
/// the handler completes. a no-op for trace context when otel is off, leaving an ordinary local span;
/// the request id still works without otel, since it is generated locally rather than derived from a
/// trace context.
async fn trace_propagation_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use tracing::Instrument;

    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let metric_method = crate::metrics::method(&method);
    let metric_route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|matched| matched.as_str().to_string())
        .unwrap_or_else(|| "unmatched".to_string());
    // reuse an inbound request id from a fronting proxy/gateway when present, so this request's logs
    // line up with that layer's; otherwise mint one so every request is correlatable even with otel off.
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

    let span = tracing::info_span!(
        "http_request",
        method = %method,
        path = %path,
        request_id = %request_id,
    );
    runinator_observability::telemetry::apply_http_context(&span, request.headers());

    async move {
        let started = std::time::Instant::now();
        let _in_flight = crate::metrics::request_started();
        let mut response = next.run(request).await;
        crate::metrics::request_completed(
            metric_method,
            &metric_route,
            response.status(),
            started.elapsed(),
        );
        let duration_ms = started.elapsed().as_millis() as u64;
        let status = response.status().as_u16();
        if status >= 500 {
            tracing::error!(status, duration_ms, "request completed");
        } else if status >= 400 {
            tracing::warn!(status, duration_ms, "request completed");
        } else {
            tracing::info!(status, duration_ms, "request completed");
        }
        if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
            response.headers_mut().insert(REQUEST_ID_HEADER, value);
        }
        response
    }
    .instrument(span)
    .await
}

/// turn a recovered handler panic into the standard json error envelope. the panic payload is logged
/// in full; the client gets a generic message so internal details are not leaked.
pub(crate) fn handle_panic(
    panic: Box<dyn std::any::Any + Send + 'static>,
) -> axum::response::Response {
    let detail = panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic payload".to_string());
    log::error!("recovered from panic in HTTP handler: {detail}");
    crate::stability::record_handler_panic();
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(ApiResponse::ApiError(ApiError::new(
            "internal server error",
        ))),
    )
        .into_response()
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
