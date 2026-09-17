#[allow(unused_imports)]
use super::*;

/// network throughput for one replica host, aggregated over all interfaces. rates are derived from
/// the byte delta and elapsed time between consecutive samples on the same collector.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NetworkTelemetry {
    /// received bytes per second since the previous sample. zero on the first sample.
    pub rx_bytes_per_sec: f64,
    /// transmitted bytes per second since the previous sample. zero on the first sample.
    pub tx_bytes_per_sec: f64,
    /// cumulative bytes received across all interfaces since the host started counting.
    pub rx_total_bytes: u64,
    /// cumulative bytes transmitted across all interfaces since the host started counting.
    pub tx_total_bytes: u64,
}
