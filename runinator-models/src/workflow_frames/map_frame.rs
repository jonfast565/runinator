#[allow(unused_imports)]
use super::*;

/// `state.map` parent fan-out bookkeeping or child item binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapFrame {
    pub node_id: String,
    pub target: String,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default = "default_concurrency")]
    pub concurrency: i64,
    #[serde(default)]
    pub next_index: i64,
    #[serde(default)]
    pub in_flight: Vec<MapChild>,
    #[serde(default)]
    pub results: Vec<Value>,
    #[serde(default)]
    pub done: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    #[serde(default)]
    pub index: i64,
}
