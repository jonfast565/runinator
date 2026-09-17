#[allow(unused_imports)]
use super::*;

pub(super) struct BrokerMetrics {
    pub(super) backend: String,
    pub(super) operations: Counter<u64>,
    pub(super) duration_ms: Histogram<f64>,
    pub(super) _connection_state: ObservableGauge<u64>,
}

impl BrokerMetrics {
    pub(super) fn new(
        backend: String,
        connection_state: Option<tokio::sync::watch::Receiver<ConnectionState>>,
    ) -> Self {
        let meter = opentelemetry::global::meter(METER_NAME);
        let callback_backend = backend.clone();
        let connection_state = meter
            .u64_observable_gauge("runinator_broker_connection_state")
            .with_callback(move |observer| {
                let connected = connection_state
                    .as_ref()
                    .is_none_or(|state| state.borrow().is_connected());
                observer.observe(
                    u64::from(connected),
                    &[KeyValue::new("backend", callback_backend.clone())],
                );
            })
            .build();
        Self {
            backend,
            operations: meter.u64_counter(METRIC_OPERATIONS).build(),
            duration_ms: meter
                .f64_histogram(METRIC_DURATION_MS)
                .with_unit("ms")
                .build(),
            _connection_state: connection_state,
        }
    }

    // record a completed operation. every call increments the operations counter tagged with the
    // outcome; `timed` operations (non-blocking publishes and acks) also record their latency, while
    // blocking receives are left untimed so the histogram never conflates idle wait with work.
    pub(super) fn record<T>(
        &self,
        channel: &'static str,
        op: &'static str,
        start: Instant,
        result: &Result<T, BrokerError>,
        timed: bool,
    ) {
        let outcome = if result.is_ok() { "ok" } else { "error" };
        self.operations.add(
            1,
            &[
                KeyValue::new("backend", self.backend.clone()),
                KeyValue::new("channel", channel),
                KeyValue::new("op", op),
                KeyValue::new("outcome", outcome),
            ],
        );
        if !timed {
            return;
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        self.duration_ms.record(
            elapsed_ms,
            &[
                KeyValue::new("backend", self.backend.clone()),
                KeyValue::new("channel", channel),
                KeyValue::new("op", op),
            ],
        );
    }
}
