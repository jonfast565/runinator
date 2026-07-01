// opentelemetry metrics for the action loop. bound lazily to the global meter so they export over
// otlp when otel is configured and are cheap no-ops otherwise. names are stable public contracts.

use std::sync::OnceLock;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};

const METER_NAME: &str = "runinator-worker";

struct WorkerMetrics {
    actions_received: Counter<u64>,
    actions_completed: Counter<u64>,
    actions_duplicate: Counter<u64>,
    action_duration_ms: Histogram<f64>,
    actions_in_flight: UpDownCounter<i64>,
    control_commands: Counter<u64>,
    secret_resolution_failures: Counter<u64>,
}

static METRICS: OnceLock<WorkerMetrics> = OnceLock::new();

fn metrics() -> &'static WorkerMetrics {
    METRICS.get_or_init(|| {
        let meter = opentelemetry::global::meter(METER_NAME);
        WorkerMetrics {
            actions_received: meter
                .u64_counter("runinator_worker_actions_received_total")
                .build(),
            actions_completed: meter
                .u64_counter("runinator_worker_actions_completed_total")
                .build(),
            actions_duplicate: meter
                .u64_counter("runinator_worker_actions_duplicate_total")
                .build(),
            action_duration_ms: meter
                .f64_histogram("runinator_worker_action_duration_ms")
                .with_unit("ms")
                .build(),
            actions_in_flight: meter
                .i64_up_down_counter("runinator_worker_actions_in_flight")
                .build(),
            control_commands: meter
                .u64_counter("runinator_worker_control_commands_total")
                .build(),
            secret_resolution_failures: meter
                .u64_counter("runinator_worker_secret_resolution_failures_total")
                .build(),
        }
    })
}

/// an action delivery was accepted for processing (before lease/dedupe checks).
pub(crate) fn action_received() {
    metrics().actions_received.add(1, &[]);
}

/// a delivery was dropped as a duplicate because its executor lease is held elsewhere.
pub(crate) fn action_duplicate() {
    metrics().actions_duplicate.add(1, &[]);
}

/// an action finished executing. `outcome` is one of succeeded/failed/timed_out/canceled; the same
/// label is applied to the duration histogram so latency can be split by result.
pub(crate) fn action_completed(outcome: &'static str, duration_ms: f64) {
    let attrs = [KeyValue::new("outcome", outcome)];
    metrics().actions_completed.add(1, &attrs);
    metrics().action_duration_ms.record(duration_ms, &attrs);
}

/// resolving `secret://` references for an action failed, so it was reported failed without running.
pub(crate) fn secret_resolution_failure() {
    metrics().secret_resolution_failures.add(1, &[]);
}

/// a control command was received on the control channel. `kind` is cancel/pause/resume.
pub(crate) fn control_command(kind: &'static str) {
    metrics()
        .control_commands
        .add(1, &[KeyValue::new("kind", kind)]);
}

/// raise the in-flight gauge for the lifetime of one executing action, lowering it on drop so every
/// exit path (including error returns) is accounted for.
pub(crate) fn in_flight_guard() -> InFlightGuard {
    metrics().actions_in_flight.add(1, &[]);
    InFlightGuard
}

pub(crate) struct InFlightGuard;

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        metrics().actions_in_flight.add(-1, &[]);
    }
}
