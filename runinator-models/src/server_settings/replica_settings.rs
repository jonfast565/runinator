#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReplicaSettings {
    pub stale_after_seconds: u64,
    pub reap_after_seconds: u64,
    pub delete_after_seconds: u64,
    pub reaper_interval_seconds: u64,
    pub sample_retention_seconds: u64,
    pub sample_window_seconds: u64,
    pub sample_max_points: u64,
}

impl Default for ReplicaSettings {
    fn default() -> Self {
        Self {
            stale_after_seconds: 30,
            reap_after_seconds: 600,
            delete_after_seconds: 3_600,
            reaper_interval_seconds: 60,
            sample_retention_seconds: 86_400,
            sample_window_seconds: 3_600,
            sample_max_points: 1_000,
        }
    }
}
