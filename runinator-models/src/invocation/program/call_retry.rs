#[allow(unused_imports)]
use super::*;

/// the retry shape a call carries, mirroring the node-level `WorkflowRetry` fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRetry {
    pub max_attempts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_base_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_max_seconds: Option<i64>,
    #[serde(default)]
    pub jitter: bool,
    /// which terminal statuses are retryable, by the same names the node-level policy uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_on: Option<String>,
}
