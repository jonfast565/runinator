// resource-usage telemetry sampled by each replica and carried on heartbeats under
// `attributes.telemetry`. kept transport-friendly so it round-trips through the replica
// registry and out the `/replicas` api without bespoke mapping.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

/// unix-style load average over 1, 5, and 15 minutes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// resource usage attributed to the replica's own process.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProcessTelemetry {
    /// process cpu utilization; may exceed 100 when the process spans multiple cores.
    pub cpu_percent: f32,
    /// resident set size of the process, in bytes.
    pub mem_used_bytes: u64,
}

/// capacity and i/o throughput for one mounted filesystem.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DiskTelemetry {
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    /// bytes read per second since the previous sample. zero on the first sample.
    pub read_bytes_per_sec: f64,
    /// bytes written per second since the previous sample. zero on the first sample.
    pub written_bytes_per_sec: f64,
}

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

/// static host facts that do not change over a process lifetime. carried once in the replica's
/// registration attributes rather than on every heartbeat.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HostMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel_version: Option<String>,
    pub cpu_arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_brand: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub mem_total_bytes: u64,
    /// host boot time as a unix timestamp in seconds.
    pub boot_time_unix: u64,
}

/// telemetry for a single gpu. fields are optional because backends expose different metrics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuTelemetry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization_percent: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_used_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_total_bytes: Option<u64>,
}
