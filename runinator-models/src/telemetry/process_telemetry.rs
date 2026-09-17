#[allow(unused_imports)]
use super::*;

/// resource usage attributed to the replica's own process.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProcessTelemetry {
    /// process cpu utilization; may exceed 100 when the process spans multiple cores.
    pub cpu_percent: f32,
    /// resident set size of the process, in bytes.
    pub mem_used_bytes: u64,
}
