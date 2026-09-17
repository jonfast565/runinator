#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct ReplicaSampleQuery {
    /// look-back window in seconds; defaults to the last hour when absent.
    pub since_seconds: Option<i64>,
}
