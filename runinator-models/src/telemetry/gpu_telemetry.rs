#[allow(unused_imports)]
use super::*;

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
