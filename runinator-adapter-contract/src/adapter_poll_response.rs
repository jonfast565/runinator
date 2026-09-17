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
}
