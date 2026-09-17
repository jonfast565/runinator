#[allow(unused_imports)]
use super::*;

/// reducer-owned per-cursor debug runtime state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DebugRuntime {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub step_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_shot_breakpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_kind: Option<WorkflowNodeKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_output_json: Option<Value>,
}
