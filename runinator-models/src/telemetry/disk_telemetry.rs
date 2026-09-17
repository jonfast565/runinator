#[allow(unused_imports)]
use super::*;

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
