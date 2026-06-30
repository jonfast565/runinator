use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use opentelemetry::metrics::Counter;
use serde::Serialize;
use utoipa::ToSchema;

static RESULT_EVENTS_APPLIED: AtomicU64 = AtomicU64::new(0);
static RESULT_EVENTS_DUPLICATE: AtomicU64 = AtomicU64::new(0);
static RESULT_EVENTS_RETRIED: AtomicU64 = AtomicU64::new(0);
static RESULT_EVENTS_DEAD_LETTERED: AtomicU64 = AtomicU64::new(0);
static RESULT_RECEIVE_ERRORS: AtomicU64 = AtomicU64::new(0);

// metric names exported through the prometheus /metrics endpoint.
const METRIC_RESULT_APPLIED: &str = "runinator_ws_result_events_applied_total";
const METRIC_RESULT_DUPLICATE: &str = "runinator_ws_result_events_duplicate_total";
const METRIC_RESULT_RETRIED: &str = "runinator_ws_result_events_retried_total";
const METRIC_RESULT_DEAD_LETTERED: &str = "runinator_ws_result_events_dead_lettered_total";
const METRIC_RESULT_RECEIVE_ERRORS: &str = "runinator_ws_result_receive_errors_total";
const METRIC_HANDLER_PANICS: &str = "runinator_ws_handler_panics_total";
const METRIC_BACKGROUND_LOOP_FAILURES: &str = "runinator_ws_background_loop_failures_total";

static PROMETHEUS: OnceLock<PrometheusHandle> = OnceLock::new();

// otel counter handles, lazily bound to the global meter so the same stability counters also export
// over otlp when otel is configured (a no-op meter otherwise). prometheus stays the source for the
// /metrics endpoint; this is an additive parallel path.
struct OtelCounters {
    result_applied: Counter<u64>,
    result_duplicate: Counter<u64>,
    result_retried: Counter<u64>,
    result_dead_lettered: Counter<u64>,
    result_receive_errors: Counter<u64>,
    handler_panics: Counter<u64>,
    background_loop_failures: Counter<u64>,
}

static OTEL_COUNTERS: OnceLock<OtelCounters> = OnceLock::new();

fn otel_counters() -> &'static OtelCounters {
    OTEL_COUNTERS.get_or_init(|| {
        let meter = opentelemetry::global::meter("runinator-ws");
        OtelCounters {
            result_applied: meter.u64_counter(METRIC_RESULT_APPLIED).build(),
            result_duplicate: meter.u64_counter(METRIC_RESULT_DUPLICATE).build(),
            result_retried: meter.u64_counter(METRIC_RESULT_RETRIED).build(),
            result_dead_lettered: meter.u64_counter(METRIC_RESULT_DEAD_LETTERED).build(),
            result_receive_errors: meter.u64_counter(METRIC_RESULT_RECEIVE_ERRORS).build(),
            handler_panics: meter.u64_counter(METRIC_HANDLER_PANICS).build(),
            background_loop_failures: meter.u64_counter(METRIC_BACKGROUND_LOOP_FAILURES).build(),
        }
    })
}

/// install the prometheus recorder once per process. safe to call repeatedly; only the first call
/// wins. must run before the result consumer starts so early increments are recorded.
pub(crate) fn init_metrics() {
    PROMETHEUS.get_or_init(|| {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        // ignore the error when a global recorder is already installed (e.g. across tests).
        let _ = metrics::set_global_recorder(recorder);
        handle
    });
}

/// render the prometheus text exposition, or an empty body if no recorder is installed.
pub(crate) fn render_metrics() -> String {
    PROMETHEUS
        .get()
        .map(PrometheusHandle::render)
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct StabilityCounters {
    pub result_events_applied: u64,
    pub result_events_duplicate: u64,
    pub result_events_retried: u64,
    pub result_events_dead_lettered: u64,
    pub result_receive_errors: u64,
}

pub(crate) fn result_event_applied(applied: bool) {
    if applied {
        RESULT_EVENTS_APPLIED.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(METRIC_RESULT_APPLIED).increment(1);
        otel_counters().result_applied.add(1, &[]);
    } else {
        RESULT_EVENTS_DUPLICATE.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(METRIC_RESULT_DUPLICATE).increment(1);
        otel_counters().result_duplicate.add(1, &[]);
    }
}

pub(crate) fn result_event_retried() {
    RESULT_EVENTS_RETRIED.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_RETRIED).increment(1);
    otel_counters().result_retried.add(1, &[]);
}

pub(crate) fn result_event_dead_lettered() {
    RESULT_EVENTS_DEAD_LETTERED.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_DEAD_LETTERED).increment(1);
    otel_counters().result_dead_lettered.add(1, &[]);
}

pub(crate) fn result_receive_error() {
    RESULT_RECEIVE_ERRORS.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_RECEIVE_ERRORS).increment(1);
    otel_counters().result_receive_errors.add(1, &[]);
}

/// a request handler panicked and was recovered by the catch-panic layer (the connection got a 500
/// instead of being dropped). exported for alerting; a nonzero rate points at a reachable panic.
pub(crate) fn record_handler_panic() {
    metrics::counter!(METRIC_HANDLER_PANICS).increment(1);
    otel_counters().handler_panics.add(1, &[]);
}

/// a background orchestration loop exited unexpectedly (panic or early return). this is fatal for the
/// replica, which shuts down so it can restart and resume from durable state rather than silently
/// stalling with a dead loop.
pub(crate) fn record_background_loop_failure() {
    metrics::counter!(METRIC_BACKGROUND_LOOP_FAILURES).increment(1);
    otel_counters().background_loop_failures.add(1, &[]);
}

pub(crate) fn snapshot() -> StabilityCounters {
    StabilityCounters {
        result_events_applied: RESULT_EVENTS_APPLIED.load(Ordering::Relaxed),
        result_events_duplicate: RESULT_EVENTS_DUPLICATE.load(Ordering::Relaxed),
        result_events_retried: RESULT_EVENTS_RETRIED.load(Ordering::Relaxed),
        result_events_dead_lettered: RESULT_EVENTS_DEAD_LETTERED.load(Ordering::Relaxed),
        result_receive_errors: RESULT_RECEIVE_ERRORS.load(Ordering::Relaxed),
    }
}
