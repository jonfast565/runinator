// OpenTelemetry metrics for the VM provider-effect loop. Bound lazily to the global meter so they export over
// otlp when otel is configured and are cheap no-ops otherwise. names are stable public contracts.

use std::sync::OnceLock;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge, Histogram, UpDownCounter};

const METER_NAME: &str = "runinator-worker";

static METRICS: OnceLock<WorkerMetrics> = OnceLock::new();

fn metrics() -> &'static WorkerMetrics {
    METRICS.get_or_init(|| {
        let meter = opentelemetry::global::meter(METER_NAME);
        WorkerMetrics {
            effects_received: meter
                .u64_counter("runinator_worker_effects_received_total")
                .build(),
            effects_completed: meter
                .u64_counter("runinator_worker_effects_completed_total")
                .build(),
            effect_duration_ms: meter
                .f64_histogram("runinator_worker_effect_duration_ms")
                .with_unit("ms")
                .build(),
            effects_in_flight: meter
                .i64_up_down_counter("runinator_worker_effects_in_flight")
                .build(),
            control_commands: meter
                .u64_counter("runinator_worker_control_commands_total")
                .build(),
            secret_resolution_failures: meter
                .u64_counter("runinator_worker_secret_resolution_failures_total")
                .build(),
            capacity: meter.u64_gauge("runinator_worker_capacity").build(),
            result_publish: meter
                .u64_counter("runinator_worker_result_publish_total")
                .build(),
        }
    })
}

/// A VM provider effect was accepted for processing.
pub(crate) fn effect_received() {
    runinator_tui::counter("worker", "effects received", 1);
    metrics().effects_received.add(1, &[]);
}

pub(crate) fn capacity(value: usize) {
    runinator_tui::gauge("worker", "action capacity", value as i64);
    metrics().capacity.record(value as u64, &[]);
}

pub(crate) fn result_publish(outcome: &'static str) {
    runinator_tui::counter("worker", "results published", 1);
    runinator_tui::activity("worker", format!("result publish {outcome}"), None);
    metrics()
        .result_publish
        .add(1, &[KeyValue::new("outcome", outcome)]);
}

/// A provider effect finished executing. `outcome` is one of succeeded/failed/timed_out/canceled; the same
/// label is applied to the duration histogram so latency can be split by result.
pub(crate) fn effect_completed(outcome: &'static str, duration_ms: f64) {
    runinator_tui::counter("worker", "effects completed", 1);
    runinator_tui::activity(
        "worker",
        format!("effect {outcome} ({duration_ms:.0} ms)"),
        None,
    );
    let attrs = [KeyValue::new("outcome", outcome)];
    metrics().effects_completed.add(1, &attrs);
    metrics().effect_duration_ms.record(duration_ms, &attrs);
}

/// Resolving `secret://` references for an effect failed, so it was reported failed without running.
pub(crate) fn secret_resolution_failure() {
    runinator_tui::counter("worker", "secret failures", 1);
    metrics().secret_resolution_failures.add(1, &[]);
}

/// a control command was received on the control channel. `kind` is cancel/pause/resume.
pub(crate) fn control_command(kind: &'static str) {
    runinator_tui::counter("worker", "control commands", 1);
    runinator_tui::activity("worker", format!("control {kind}"), None);
    metrics()
        .control_commands
        .add(1, &[KeyValue::new("kind", kind)]);
}

/// Raise the in-flight gauge for the lifetime of one executing effect, lowering it on drop so every
/// exit path (including error returns) is accounted for.
pub(crate) fn in_flight_guard() -> InFlightGuard {
    runinator_tui::gauge_increment("worker", "effects in flight", 1);
    metrics().effects_in_flight.add(1, &[]);
    InFlightGuard
}

mod worker_metrics;
use worker_metrics::WorkerMetrics;

mod in_flight_guard;
pub(crate) use in_flight_guard::InFlightGuard;
