// opentelemetry metrics for the wake relay loop. bound lazily to the global meter so they export
// over otlp when otel is configured and are cheap no-ops otherwise. names are stable public contracts.

use std::sync::OnceLock;

use opentelemetry::metrics::{Counter, Histogram};

const METER_NAME: &str = "runinator-waker";

struct WakerMetrics {
    wakes_received: Counter<u64>,
    wakes_driven: Counter<u64>,
    wakes_requeued: Counter<u64>,
    drive_failures: Counter<u64>,
    broker_heartbeats: Counter<u64>,
    broker_heartbeat_failures: Counter<u64>,
    wake_lead_ms: Histogram<f64>,
    wake_due_lag_ms: Histogram<f64>,
}

static METRICS: OnceLock<WakerMetrics> = OnceLock::new();

fn metrics() -> &'static WakerMetrics {
    METRICS.get_or_init(|| {
        let meter = opentelemetry::global::meter(METER_NAME);
        WakerMetrics {
            wakes_received: meter
                .u64_counter("runinator_waker_wakes_received_total")
                .build(),
            wakes_driven: meter
                .u64_counter("runinator_waker_wakes_driven_total")
                .build(),
            wakes_requeued: meter
                .u64_counter("runinator_waker_wakes_requeued_total")
                .build(),
            drive_failures: meter
                .u64_counter("runinator_waker_drive_failures_total")
                .build(),
            broker_heartbeats: meter
                .u64_counter("runinator_waker_broker_heartbeats_total")
                .build(),
            broker_heartbeat_failures: meter
                .u64_counter("runinator_waker_broker_heartbeat_failures_total")
                .build(),
            wake_lead_ms: meter
                .f64_histogram("runinator_waker_wake_lead_ms")
                .with_unit("ms")
                .build(),
            wake_due_lag_ms: meter
                .f64_histogram("runinator_waker_due_lag_ms")
                .with_unit("ms")
                .build(),
        }
    })
}

/// a wake was pulled off the wake channel. `lead_ms` is how far in the future its `due_at` is at
/// receipt (negative when already overdue), recorded so scheduling lead/lag is observable.
pub(crate) fn wake_received(lead_ms: f64) {
    metrics().wakes_received.add(1, &[]);
    metrics().wake_lead_ms.record(lead_ms, &[]);
}

pub(crate) fn wake_due_lag(lag_ms: f64) {
    metrics().wake_due_lag_ms.record(lag_ms.max(0.0), &[]);
}

/// a due wake was relayed to the ingress channel as an effect settle (or was already in flight).
/// the exported metric name predates the settle payload and is kept as a stable contract.
pub(crate) fn wake_driven() {
    metrics().wakes_driven.add(1, &[]);
}

/// a not-yet-due wake was returned to the broker for later redelivery.
pub(crate) fn wake_requeued() {
    metrics().wakes_requeued.add(1, &[]);
}

/// publishing the settle for a due wake failed; it was returned to the broker to retry.
pub(crate) fn drive_failed() {
    metrics().drive_failures.add(1, &[]);
}

/// the relay completed a broker transport heartbeat while idle or active.
pub(crate) fn broker_heartbeat() {
    metrics().broker_heartbeats.add(1, &[]);
}

/// the broker transport did not answer a heartbeat. The relay keeps retrying its normal receive.
pub(crate) fn broker_heartbeat_failed() {
    metrics().broker_heartbeat_failures.add(1, &[]);
}
