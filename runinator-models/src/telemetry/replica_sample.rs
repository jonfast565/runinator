#[allow(unused_imports)]
use super::*;

/// a flattened, persisted telemetry point for one replica, stored in the `replica_samples`
/// Time-series data used by the UI for historical sparklines.
/// Keep only fields worth charting; full nested telemetry stays in live replica `attributes`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplicaSample {
    pub replica_id: uuid::Uuid,
    pub sampled_at: DateTime<Utc>,
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    /// 1-minute load average, when reported by the host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_one: Option<f64>,
    pub process_cpu_percent: f32,
    pub process_mem_bytes: u64,
    pub net_rx_bytes_per_sec: f64,
    pub net_tx_bytes_per_sec: f64,
}

impl ReplicaSample {
    /// derive a persisted sample from a live telemetry snapshot.
    pub fn from_telemetry(replica_id: uuid::Uuid, telemetry: &ResourceTelemetry) -> Self {
        Self {
            replica_id,
            sampled_at: telemetry.sampled_at,
            cpu_percent: telemetry.cpu_percent,
            mem_percent: telemetry.mem_percent,
            mem_used_bytes: telemetry.mem_used_bytes,
            mem_total_bytes: telemetry.mem_total_bytes,
            load_one: telemetry.load_average.as_ref().map(|load| load.one),
            process_cpu_percent: telemetry.process.cpu_percent,
            process_mem_bytes: telemetry.process.mem_used_bytes,
            net_rx_bytes_per_sec: telemetry.network.rx_bytes_per_sec,
            net_tx_bytes_per_sec: telemetry.network.tx_bytes_per_sec,
        }
    }
}
