#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterPollResponse {
    #[serde(default)]
    pub events: Vec<NormalizedAdapterEvent>,
    #[serde(default)]
    pub checkpoint: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Build version of the adapter host that produced this batch. A worker resolves its host
    /// binary from the environment, from its own directory, and finally from `$PATH`, so the one
    /// that answered is not always the one that shipped with the engine; naming it is what turns a
    /// day-stale binary from an invisible difference in output into something an operator can read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_version: Option<String>,
    /// Version of the adapter kind as the running host implements it, checked against the version
    /// the adapter revision pinned at apply time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_version: Option<String>,
}
