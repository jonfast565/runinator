#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRetentionConfig {
    #[serde(default = "default_log_retention_max_age_days")]
    pub max_age_days: u64,
    #[serde(default = "default_log_retention_max_files")]
    pub max_files: usize,
    #[serde(default = "default_log_retention_max_bytes")]
    pub max_bytes: u64,
}

impl Default for LogRetentionConfig {
    fn default() -> Self {
        Self {
            max_age_days: default_log_retention_max_age_days(),
            max_files: default_log_retention_max_files(),
            max_bytes: default_log_retention_max_bytes(),
        }
    }
}
