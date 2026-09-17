#[allow(unused_imports)]
use super::*;

/// one visited node in a simulation trace.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimStep {
    pub node_id: String,
    pub kind: WorkflowNodeKind,
    pub status: WorkflowStatus,
    /// the next node the walk routed to, when the node had an outgoing edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// the value recorded as this node's output, when it produced one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    /// a short reason string mirroring the reducer's transition reasons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}
