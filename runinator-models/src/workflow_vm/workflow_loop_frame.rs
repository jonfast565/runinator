#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowLoopFrame {
    pub loop_key: String,
    pub body: usize,
    pub exit: usize,
    #[serde(default)]
    pub index: u64,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub results: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u64>,
}
