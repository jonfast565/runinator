#[allow(unused_imports)]
use super::*;

/// `.retry(max, backoff: <s>, max: <s>, jitter: <bool>, on: any|failure|timeout)`. only `max` is
/// required; the rest fall back to the model defaults (base 1s, cap 300s, no jitter, retry any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryConfig {
    pub max_attempts: i64,
    pub backoff_base_seconds: Option<i64>,
    pub backoff_max_seconds: Option<i64>,
    pub jitter: bool,
    /// `any` | `failure` | `timeout`; `None` keeps the default (`any`).
    pub retry_on: Option<String>,
}
