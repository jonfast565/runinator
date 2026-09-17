#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowMapFrame {
    pub map_key: String,
    pub body: usize,
    pub exit: usize,
    pub concurrency: u64,
    #[serde(default)]
    pub next_index: u64,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub results: Vec<WorkflowIndexedValue>,
    /// The item carried by a child continuation. Its index is enough to order the parent result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_index: Option<u64>,
}
