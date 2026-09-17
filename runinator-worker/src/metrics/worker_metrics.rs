#[allow(unused_imports)]
use super::*;

pub(super) struct WorkerMetrics {
    pub(super) effects_received: Counter<u64>,
    pub(super) effects_completed: Counter<u64>,
    pub(super) effect_duration_ms: Histogram<f64>,
    pub(super) effects_in_flight: UpDownCounter<i64>,
    pub(super) control_commands: Counter<u64>,
    pub(super) secret_resolution_failures: Counter<u64>,
    pub(super) capacity: Gauge<u64>,
    pub(super) result_publish: Counter<u64>,
}
