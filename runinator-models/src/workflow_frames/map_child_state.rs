#[allow(unused_imports)]
use super::*;

/// child-run marker stored under `state.map_child`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapChildState {
    pub stop_node: String,
    pub index: i64,
    pub item: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}
