#[allow(unused_imports)]
use super::*;

/// `state.try` phase bookkeeping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TryFrame {
    pub node_id: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_status: Option<WorkflowStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_output: Option<Value>,
}
