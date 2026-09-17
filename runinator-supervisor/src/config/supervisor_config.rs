#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SupervisorConfig {
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
    #[serde(default = "default_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
    #[serde(default = "default_restart_delay_ms")]
    pub restart_delay_ms: u64,
    #[serde(default)]
    pub log_retention: LogRetentionConfig,
    #[serde(default)]
    pub processes: Vec<ProcessConfig>,
}
