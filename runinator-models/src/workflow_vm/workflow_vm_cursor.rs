#[allow(unused_imports)]
use super::*;

/// The graph-facing view of a durable continuation.  Execution identity stays the continuation
/// id; the node id is derived only for rendering from the frozen module source map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowVmCursor {
    pub continuation_id: Uuid,
    pub instruction_pointer: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_label: Option<String>,
    pub status: WorkflowContinuationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_to_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_failure: Option<WorkflowFailure>,
}
