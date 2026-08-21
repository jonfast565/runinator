use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge, Histogram};
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
const METRIC_INGRESS_APPLIED: &str = "runinator_ws_ingress_applied_total";
const METRIC_INGRESS_RETRIED: &str = "runinator_ws_ingress_retried_total";
const METRIC_INGRESS_DEAD_LETTERED: &str = "runinator_ws_ingress_dead_lettered_total";
const METRIC_TRIGGERS_FIRED: &str = "runinator_ws_triggers_fired_total";
const METRIC_REDUCER_DRIVE_MS: &str = "runinator_ws_reducer_drive_ms";
const METRIC_VM_CONTINUATIONS_DRIVEN: &str = "runinator_vm_continuations_driven_total";
const METRIC_VM_DRIVE_DURATION_MS: &str = "runinator_vm_drive_duration_ms";
const METRIC_VM_DRIVER_FAILURES: &str = "runinator_vm_driver_failures_total";
const METRIC_LOOP_ITERATIONS: &str = "runinator_engine_loop_iterations_total";
const METRIC_LOOP_DURATION_MS: &str = "runinator_engine_loop_duration_ms";
const METRIC_LOOP_LAST_SUCCESS: &str = "runinator_engine_loop_last_success_unixtime";
const METRIC_CLEANUP: &str = "runinator_engine_cleanup_total";
const METRIC_QUEUE_DEPTH: &str = "runinator_engine_queue_depth";
const METRIC_QUEUE_OLDEST_AGE: &str = "runinator_engine_queue_oldest_age_seconds";
const METRIC_QUEUE_CLAIMED: &str = "runinator_engine_queue_claimed";
const METRIC_QUEUE_FAILURES: &str = "runinator_engine_queue_failures_total";
const METRIC_REPLICAS: &str = "runinator_engine_replicas";
const METRIC_REPLICA_HEARTBEAT_AGE: &str = "runinator_engine_replica_max_heartbeat_age_seconds";
const METRIC_REPLICA_TRANSITIONS: &str = "runinator_engine_replica_transitions_total";

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
    ingress_applied: Counter<u64>,
    ingress_retried: Counter<u64>,
    ingress_dead_lettered: Counter<u64>,
    triggers_fired: Counter<u64>,
    reducer_drive_ms: Histogram<f64>,
    vm_continuations_driven: Counter<u64>,
    vm_drive_duration_ms: Histogram<f64>,
    vm_driver_failures: Counter<u64>,
    loop_iterations: Counter<u64>,
    loop_duration_ms: Histogram<f64>,
    loop_last_success: Gauge<u64>,
    cleanup: Counter<u64>,
    queue_depth: Gauge<u64>,
    queue_oldest_age: Gauge<u64>,
    queue_claimed: Gauge<u64>,
    queue_failures: Counter<u64>,
    replicas: Gauge<u64>,
    replica_heartbeat_age: Gauge<u64>,
    replica_transitions: Counter<u64>,
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
            ingress_applied: meter.u64_counter(METRIC_INGRESS_APPLIED).build(),
            ingress_retried: meter.u64_counter(METRIC_INGRESS_RETRIED).build(),
            ingress_dead_lettered: meter.u64_counter(METRIC_INGRESS_DEAD_LETTERED).build(),
            triggers_fired: meter.u64_counter(METRIC_TRIGGERS_FIRED).build(),
            reducer_drive_ms: meter
                .f64_histogram(METRIC_REDUCER_DRIVE_MS)
                .with_unit("ms")
                .build(),
            vm_continuations_driven: meter.u64_counter(METRIC_VM_CONTINUATIONS_DRIVEN).build(),
            vm_drive_duration_ms: meter
                .f64_histogram(METRIC_VM_DRIVE_DURATION_MS)
                .with_unit("ms")
                .build(),
            vm_driver_failures: meter.u64_counter(METRIC_VM_DRIVER_FAILURES).build(),
            loop_iterations: meter.u64_counter(METRIC_LOOP_ITERATIONS).build(),
            loop_duration_ms: meter
                .f64_histogram(METRIC_LOOP_DURATION_MS)
                .with_unit("ms")
                .build(),
            loop_last_success: meter
                .u64_gauge(METRIC_LOOP_LAST_SUCCESS)
                .with_unit("s")
                .build(),
            cleanup: meter.u64_counter(METRIC_CLEANUP).build(),
            queue_depth: meter.u64_gauge(METRIC_QUEUE_DEPTH).build(),
            queue_oldest_age: meter
                .u64_gauge(METRIC_QUEUE_OLDEST_AGE)
                .with_unit("s")
                .build(),
            queue_claimed: meter.u64_gauge(METRIC_QUEUE_CLAIMED).build(),
            queue_failures: meter.u64_counter(METRIC_QUEUE_FAILURES).build(),
            replicas: meter.u64_gauge(METRIC_REPLICAS).build(),
            replica_heartbeat_age: meter
                .u64_gauge(METRIC_REPLICA_HEARTBEAT_AGE)
                .with_unit("s")
                .build(),
            replica_transitions: meter.u64_counter(METRIC_REPLICA_TRANSITIONS).build(),
        }
    })
}

/// install the prometheus recorder once per process. safe to call repeatedly; only the first call
/// wins. must run before the result consumer starts so early increments are recorded.
pub fn init_metrics() {
    PROMETHEUS.get_or_init(|| {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        // ignore the error when a global recorder is already installed (e.g. across tests).
        let _ = metrics::set_global_recorder(recorder);
        handle
    });
}

/// render the prometheus text exposition, or an empty body if no recorder is installed.
pub fn render_metrics() -> String {
    PROMETHEUS
        .get()
        .map(PrometheusHandle::render)
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StabilityCounters {
    pub result_events_applied: u64,
    pub result_events_duplicate: u64,
    pub result_events_retried: u64,
    pub result_events_dead_lettered: u64,
    pub result_receive_errors: u64,
}

pub fn result_event_applied(applied: bool) {
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

pub fn result_event_retried() {
    RESULT_EVENTS_RETRIED.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_RETRIED).increment(1);
    otel_counters().result_retried.add(1, &[]);
}

pub fn result_event_dead_lettered() {
    RESULT_EVENTS_DEAD_LETTERED.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_DEAD_LETTERED).increment(1);
    otel_counters().result_dead_lettered.add(1, &[]);
}

pub fn result_receive_error() {
    RESULT_RECEIVE_ERRORS.fetch_add(1, Ordering::Relaxed);
    metrics::counter!(METRIC_RESULT_RECEIVE_ERRORS).increment(1);
    otel_counters().result_receive_errors.add(1, &[]);
}

/// a request handler panicked and was recovered by the catch-panic layer (the connection got a 500
/// instead of being dropped). exported for alerting; a nonzero rate points at a reachable panic.
pub fn record_handler_panic() {
    metrics::counter!(METRIC_HANDLER_PANICS).increment(1);
    otel_counters().handler_panics.add(1, &[]);
}

/// a background orchestration loop exited unexpectedly (panic or early return). this is fatal for the
/// replica, which shuts down so it can restart and resume from durable state rather than silently
/// stalling with a dead loop.
pub fn record_background_loop_failure() {
    metrics::counter!(METRIC_BACKGROUND_LOOP_FAILURES).increment(1);
    otel_counters().background_loop_failures.add(1, &[]);
}

/// an ingress message (a waker drive or worker control request) was applied and acked.
pub fn ingress_applied() {
    metrics::counter!(METRIC_INGRESS_APPLIED).increment(1);
    otel_counters().ingress_applied.add(1, &[]);
}

/// an ingress message failed and was returned to the broker for another attempt.
pub fn ingress_retried() {
    metrics::counter!(METRIC_INGRESS_RETRIED).increment(1);
    otel_counters().ingress_retried.add(1, &[]);
}

/// an ingress message exhausted its attempts and was dead-lettered. a nonzero rate points at a
/// persistently failing reducer drive or control request.
pub fn ingress_dead_lettered() {
    metrics::counter!(METRIC_INGRESS_DEAD_LETTERED).increment(1);
    otel_counters().ingress_dead_lettered.add(1, &[]);
}

/// `count` due workflow triggers were claimed and turned into runs in one trigger-loop iteration.
pub fn triggers_fired(count: u64) {
    if count == 0 {
        return;
    }
    metrics::counter!(METRIC_TRIGGERS_FIRED).increment(count);
    otel_counters().triggers_fired.add(count, &[]);
}

/// record the wall-clock time the reducer spent advancing a run for one ingress drive, in
/// milliseconds. surfaces reducer latency independent of broker/queue wait.
pub fn record_reducer_drive_ms(millis: f64) {
    metrics::histogram!(METRIC_REDUCER_DRIVE_MS).record(millis);
    otel_counters().reducer_drive_ms.record(millis, &[]);
}

/// Record one continuation the durable VM drove. `outcome` is a fixed VM result, never a
/// workflow, cursor, or provider identifier, so dashboard cardinality remains bounded.
pub fn vm_continuation_driven(outcome: &'static str) {
    metrics::counter!(METRIC_VM_CONTINUATIONS_DRIVEN, "outcome" => outcome).increment(1);
    otel_counters()
        .vm_continuations_driven
        .add(1, &[KeyValue::new("outcome", outcome)]);
}

/// Record the time spent claiming and advancing a batch of runnable VM continuations, excluding
/// the driver's polling sleep. This makes scheduler and store latency visible separately from
/// worker effect execution.
pub fn record_vm_drive_duration_ms(millis: f64) {
    metrics::histogram!(METRIC_VM_DRIVE_DURATION_MS).record(millis);
    otel_counters().vm_drive_duration_ms.record(millis, &[]);
}

/// A VM drive batch could not be claimed or advanced. Pipeline reconciliation failures are
/// intentionally excluded: they are follow-up orchestration, not interpreter failures.
pub fn vm_driver_failure() {
    metrics::counter!(METRIC_VM_DRIVER_FAILURES).increment(1);
    otel_counters().vm_driver_failures.add(1, &[]);
}

/// Record one bounded background-loop iteration. Callers pass only constants declared beside the
/// loop; identifiers and error messages never become metric attributes.
pub fn loop_iteration(loop_name: &'static str, succeeded: bool, elapsed: std::time::Duration) {
    let outcome = if succeeded { "success" } else { "error" };
    let attrs = [
        KeyValue::new("loop", loop_name),
        KeyValue::new("outcome", outcome),
    ];
    let millis = elapsed.as_secs_f64() * 1000.0;
    metrics::counter!(METRIC_LOOP_ITERATIONS, "loop" => loop_name, "outcome" => outcome)
        .increment(1);
    metrics::histogram!(METRIC_LOOP_DURATION_MS, "loop" => loop_name).record(millis);
    otel_counters().loop_iterations.add(1, &attrs);
    otel_counters()
        .loop_duration_ms
        .record(millis, &[KeyValue::new("loop", loop_name)]);
    if succeeded {
        let timestamp = chrono::Utc::now().timestamp().max(0) as u64;
        metrics::gauge!(METRIC_LOOP_LAST_SUCCESS, "loop" => loop_name).set(timestamp as f64);
        otel_counters()
            .loop_last_success
            .record(timestamp, &[KeyValue::new("loop", loop_name)]);
    }
}

pub fn cleanup(job: &'static str, succeeded: bool, count: u64) {
    let outcome = if succeeded { "success" } else { "error" };
    metrics::counter!(METRIC_CLEANUP, "job" => job, "outcome" => outcome).increment(count.max(1));
    otel_counters().cleanup.add(
        count.max(1),
        &[KeyValue::new("job", job), KeyValue::new("outcome", outcome)],
    );
}

pub fn queue_snapshot(queue: &'static str, depth: u64, claimed: u64, oldest_age_seconds: u64) {
    let attrs = [KeyValue::new("queue", queue)];
    metrics::gauge!(METRIC_QUEUE_DEPTH, "queue" => queue).set(depth as f64);
    metrics::gauge!(METRIC_QUEUE_CLAIMED, "queue" => queue).set(claimed as f64);
    metrics::gauge!(METRIC_QUEUE_OLDEST_AGE, "queue" => queue).set(oldest_age_seconds as f64);
    otel_counters().queue_depth.record(depth, &attrs);
    otel_counters().queue_claimed.record(claimed, &attrs);
    otel_counters()
        .queue_oldest_age
        .record(oldest_age_seconds, &attrs);
}

pub fn queue_failure(queue: &'static str, operation: &'static str) {
    metrics::counter!(METRIC_QUEUE_FAILURES, "queue" => queue, "operation" => operation)
        .increment(1);
    otel_counters().queue_failures.add(
        1,
        &[
            KeyValue::new("queue", queue),
            KeyValue::new("operation", operation),
        ],
    );
}

pub fn replica_snapshot(kind: &'static str, status: &'static str, count: u64) {
    metrics::gauge!(METRIC_REPLICAS, "kind" => kind, "status" => status).set(count as f64);
    otel_counters().replicas.record(
        count,
        &[KeyValue::new("kind", kind), KeyValue::new("status", status)],
    );
}

pub fn replica_heartbeat_age(kind: &'static str, age_seconds: u64) {
    metrics::gauge!(METRIC_REPLICA_HEARTBEAT_AGE, "kind" => kind).set(age_seconds as f64);
    otel_counters()
        .replica_heartbeat_age
        .record(age_seconds, &[KeyValue::new("kind", kind)]);
}

pub fn replica_transition(kind: &'static str, transition: &'static str, count: u64) {
    if count == 0 {
        return;
    }
    metrics::counter!(METRIC_REPLICA_TRANSITIONS, "kind" => kind, "transition" => transition)
        .increment(count);
    otel_counters().replica_transitions.add(
        count,
        &[
            KeyValue::new("kind", kind),
            KeyValue::new("transition", transition),
        ],
    );
}

pub fn snapshot() -> StabilityCounters {
    StabilityCounters {
        result_events_applied: RESULT_EVENTS_APPLIED.load(Ordering::Relaxed),
        result_events_duplicate: RESULT_EVENTS_DUPLICATE.load(Ordering::Relaxed),
        result_events_retried: RESULT_EVENTS_RETRIED.load(Ordering::Relaxed),
        result_events_dead_lettered: RESULT_EVENTS_DEAD_LETTERED.load(Ordering::Relaxed),
        result_receive_errors: RESULT_RECEIVE_ERRORS.load(Ordering::Relaxed),
    }
}
