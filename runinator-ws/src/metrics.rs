//! Low-cardinality HTTP and WebSocket operational metrics.

use std::sync::OnceLock;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};

const HTTP_REQUESTS: &str = "runinator_ws_http_requests_total";
const HTTP_DURATION: &str = "runinator_ws_http_request_duration_ms";
const HTTP_IN_FLIGHT: &str = "runinator_ws_http_requests_in_flight";
const HTTP_REJECTIONS: &str = "runinator_ws_request_rejections_total";
const WS_CONNECTIONS: &str = "runinator_ws_websocket_connections";
const WS_CONNECTIONS_TOTAL: &str = "runinator_ws_websocket_connections_total";

struct WsMetrics {
    requests: Counter<u64>,
    duration_ms: Histogram<f64>,
    in_flight: UpDownCounter<i64>,
    rejections: Counter<u64>,
    websocket_connections: UpDownCounter<i64>,
    websocket_connections_total: Counter<u64>,
}

static METRICS: OnceLock<WsMetrics> = OnceLock::new();

fn handles() -> &'static WsMetrics {
    METRICS.get_or_init(|| {
        let meter = opentelemetry::global::meter("runinator-ws");
        WsMetrics {
            requests: meter.u64_counter(HTTP_REQUESTS).build(),
            duration_ms: meter.f64_histogram(HTTP_DURATION).with_unit("ms").build(),
            in_flight: meter.i64_up_down_counter(HTTP_IN_FLIGHT).build(),
            rejections: meter.u64_counter(HTTP_REJECTIONS).build(),
            websocket_connections: meter.i64_up_down_counter(WS_CONNECTIONS).build(),
            websocket_connections_total: meter.u64_counter(WS_CONNECTIONS_TOTAL).build(),
        }
    })
}

pub(crate) fn method(method: &axum::http::Method) -> &'static str {
    match method.as_str() {
        "GET" => "GET",
        "POST" => "POST",
        "PUT" => "PUT",
        "PATCH" => "PATCH",
        "DELETE" => "DELETE",
        "HEAD" => "HEAD",
        "OPTIONS" => "OPTIONS",
        "CONNECT" => "CONNECT",
        "TRACE" => "TRACE",
        _ => "OTHER",
    }
}

fn status_class(status: axum::http::StatusCode) -> &'static str {
    match status.as_u16() / 100 {
        1 => "1xx",
        2 => "2xx",
        3 => "3xx",
        4 => "4xx",
        _ => "5xx",
    }
}

fn rejection_reason(status: axum::http::StatusCode) -> Option<&'static str> {
    match status {
        axum::http::StatusCode::UNAUTHORIZED => Some("unauthorized"),
        axum::http::StatusCode::FORBIDDEN => Some("forbidden"),
        axum::http::StatusCode::TOO_MANY_REQUESTS => Some("rate_limited"),
        axum::http::StatusCode::REQUEST_TIMEOUT => Some("timeout"),
        axum::http::StatusCode::PAYLOAD_TOO_LARGE => Some("body_too_large"),
        axum::http::StatusCode::SERVICE_UNAVAILABLE => Some("overloaded"),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR => Some("internal"),
        _ => None,
    }
}

pub(crate) fn request_started() -> RequestGuard {
    runinator_observability::tui::gauge_increment("web service", "HTTP in flight", 1);
    metrics::gauge!(HTTP_IN_FLIGHT).increment(1.0);
    handles().in_flight.add(1, &[]);
    RequestGuard
}

pub(crate) struct RequestGuard;

impl Drop for RequestGuard {
    fn drop(&mut self) {
        runinator_observability::tui::gauge_increment("web service", "HTTP in flight", -1);
        metrics::gauge!(HTTP_IN_FLIGHT).decrement(1.0);
        handles().in_flight.add(-1, &[]);
    }
}

pub(crate) fn request_completed(
    method: &'static str,
    route: &str,
    status: axum::http::StatusCode,
    elapsed: std::time::Duration,
) {
    let class = status_class(status);
    let duration_ms = elapsed.as_secs_f64() * 1000.0;
    runinator_observability::tui::counter("web service", "HTTP requests", 1);
    runinator_observability::tui::activity(
        "web service",
        format!("{method} {route} → {class} ({duration_ms:.0} ms)"),
        None,
    );
    let attrs = [
        KeyValue::new("method", method),
        KeyValue::new("route", route.to_string()),
        KeyValue::new("status_class", class),
    ];
    metrics::counter!(HTTP_REQUESTS, "method" => method, "route" => route.to_string(), "status_class" => class).increment(1);
    metrics::histogram!(HTTP_DURATION, "method" => method, "route" => route.to_string())
        .record(duration_ms);
    handles().requests.add(1, &attrs);
    handles().duration_ms.record(duration_ms, &attrs[..2]);
    if let Some(reason) = rejection_reason(status) {
        runinator_observability::tui::counter("web service", "HTTP rejections", 1);
        metrics::counter!(HTTP_REJECTIONS, "reason" => reason).increment(1);
        handles()
            .rejections
            .add(1, &[KeyValue::new("reason", reason)]);
    }
}

pub(crate) fn websocket_connected(kind: &'static str) -> WebSocketGuard {
    runinator_observability::tui::gauge_increment("web service", "WebSockets", 1);
    runinator_observability::tui::counter("web service", "WebSockets opened", 1);
    runinator_observability::tui::activity(
        "web service",
        format!("WebSocket {kind} connected"),
        None,
    );
    let attrs = [KeyValue::new("kind", kind)];
    metrics::gauge!(WS_CONNECTIONS, "kind" => kind).increment(1.0);
    metrics::counter!(WS_CONNECTIONS_TOTAL, "kind" => kind, "outcome" => "opened").increment(1);
    handles().websocket_connections.add(1, &attrs);
    handles().websocket_connections_total.add(
        1,
        &[
            KeyValue::new("kind", kind),
            KeyValue::new("outcome", "opened"),
        ],
    );
    WebSocketGuard { kind }
}

pub(crate) struct WebSocketGuard {
    kind: &'static str,
}

impl Drop for WebSocketGuard {
    fn drop(&mut self) {
        runinator_observability::tui::gauge_increment("web service", "WebSockets", -1);
        metrics::gauge!(WS_CONNECTIONS, "kind" => self.kind).decrement(1.0);
        metrics::counter!(WS_CONNECTIONS_TOTAL, "kind" => self.kind, "outcome" => "closed")
            .increment(1);
        handles()
            .websocket_connections
            .add(-1, &[KeyValue::new("kind", self.kind)]);
        handles().websocket_connections_total.add(
            1,
            &[
                KeyValue::new("kind", self.kind),
                KeyValue::new("outcome", "closed"),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_http_methods_are_bounded() {
        let custom = axum::http::Method::from_bytes(b"CUSTOM").unwrap();
        assert_eq!(method(&custom), "OTHER");
    }

    #[test]
    fn rejection_reasons_are_closed() {
        assert_eq!(
            rejection_reason(axum::http::StatusCode::TOO_MANY_REQUESTS),
            Some("rate_limited")
        );
        assert_eq!(rejection_reason(axum::http::StatusCode::NOT_FOUND), None);
    }
}
