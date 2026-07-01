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
    wake_lead_ms: Histogram<f64>,
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
            wake_lead_ms: meter
                .f64_histogram("runinator_waker_wake_lead_ms")
                .with_unit("ms")
                .build(),
        }
    })
}

/// a wake was pulled off the wake channel. `lead_ms` is how far in the future its `ready_at` is at
/// receipt (negative when already overdue), recorded so scheduling lead/lag is observable.
pub(crate) fn wake_received(lead_ms: f64) {
    metrics().wakes_received.add(1, &[]);
    metrics().wake_lead_ms.record(lead_ms, &[]);
}

/// a due wake was relayed to the ingress channel as a drive (or was already in flight).
pub(crate) fn wake_driven() {
    metrics().wakes_driven.add(1, &[]);
}

/// a not-yet-due wake was returned to the broker for later redelivery.
pub(crate) fn wake_requeued() {
    metrics().wakes_requeued.add(1, &[]);
}

/// publishing the drive for a due wake failed; it was returned to the broker to retry.
pub(crate) fn drive_failed() {
    metrics().drive_failures.add(1, &[]);
}
