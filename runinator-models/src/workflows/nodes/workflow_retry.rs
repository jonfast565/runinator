#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRetry {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: i64,
    /// first-retry delay in seconds; doubles each subsequent attempt up to `backoff_max_seconds`.
    #[serde(default = "default_backoff_base_seconds")]
    pub backoff_base_seconds: i64,
    /// upper bound on the computed backoff delay in seconds.
    #[serde(default = "default_backoff_max_seconds")]
    pub backoff_max_seconds: i64,
    /// when true, the computed delay is randomized in `[delay/2, delay]` to spread retry storms.
    #[serde(default)]
    pub jitter: bool,
    /// which terminal statuses are eligible for retry. defaults to retrying both failures and
    /// timeouts; narrow it so, e.g., a long expensive action is not blindly re-run on timeout.
    #[serde(default)]
    pub retry_on: WorkflowRetryClass,
}

impl Default for WorkflowRetry {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            backoff_base_seconds: default_backoff_base_seconds(),
            backoff_max_seconds: default_backoff_max_seconds(),
            jitter: false,
            retry_on: WorkflowRetryClass::default(),
        }
    }
}
