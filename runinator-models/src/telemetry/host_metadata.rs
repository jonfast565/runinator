#[allow(unused_imports)]
use super::*;

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
