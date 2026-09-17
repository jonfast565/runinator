#[allow(unused_imports)]
use super::*;

/// platform defaults applied by standalone workers. desktop agents keep their machine-local
/// settings because their capacity and lifecycle are controlled by the person running them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerSettings {
    pub max_concurrent_actions: u64,
    pub shutdown_grace_seconds: u64,
    pub reconnect_max_attempts: u64,
    pub settings_refresh_interval_seconds: u64,
}

impl Default for WorkerSettings {
    fn default() -> Self {
        Self {
            max_concurrent_actions: 4,
            shutdown_grace_seconds: 30,
            reconnect_max_attempts: 0,
            settings_refresh_interval_seconds: 5,
        }
    }
}
