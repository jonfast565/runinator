use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
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

static PROMETHEUS: OnceLock<PrometheusHandle> = OnceLock::new();

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
    } else {
        RESULT_EVENTS_DUPLICATE.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(METRIC_RESULT_DUPLICATE).increment(1);
    }
}

pub(crate) fn result_event_retried() {
    RESULT_EVENTS_RETRIED.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_RETRIED).increment(1);
}

pub(crate) fn result_event_dead_lettered() {
    RESULT_EVENTS_DEAD_LETTERED.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_DEAD_LETTERED).increment(1);
}

pub(crate) fn result_receive_error() {
    RESULT_RECEIVE_ERRORS.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_RECEIVE_ERRORS).increment(1);
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
