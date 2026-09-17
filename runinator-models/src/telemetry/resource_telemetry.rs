#[allow(unused_imports)]
use super::*;

/// a point-in-time resource snapshot for one replica process/host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceTelemetry {
    /// overall cpu utilization across all cores, 0-100.
    pub cpu_percent: f32,
    /// resident memory in use on the host, in bytes.
    pub mem_used_bytes: u64,
    /// total memory available on the host, in bytes.
    pub mem_total_bytes: u64,
    /// memory utilization, 0-100.
    pub mem_percent: f32,
    /// swap in use on the host, in bytes.
    #[serde(default)]
    pub swap_used_bytes: u64,
    /// total swap configured on the host, in bytes.
    #[serde(default)]
    pub swap_total_bytes: u64,
    /// 1/5/15-minute load average; absent on platforms that do not report it (e.g. windows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_average: Option<LoadAverage>,
    /// cpu/memory for this replica's own process, isolating it from noisy neighbors on the host.
    #[serde(default)]
    pub process: ProcessTelemetry,
    /// network throughput, summed across interfaces, since the previous sample.
    #[serde(default)]
    pub network: NetworkTelemetry,
    /// per-mount disk capacity and i/o throughput since the previous sample.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<DiskTelemetry>,
    /// per-gpu telemetry; empty when no gpu is present or no backend is available.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpus: Vec<GpuTelemetry>,
    pub sampled_at: DateTime<Utc>,
}
