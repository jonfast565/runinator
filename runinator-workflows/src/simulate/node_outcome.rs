#[allow(unused_imports)]
use super::*;

/// how a task or parked node resolved in a simulation. decouples the state-machine walk from any
/// concrete backend (a mock spec, or a database-backed replay).
#[derive(Debug, Clone, PartialEq)]
pub struct NodeOutcome {
    /// the terminal status the node reached.
    pub status: WorkflowStatus,
    /// the value recorded as the node's `output` (addressable downstream as `steps.<id>.output`).
    pub output: Value,
}

impl NodeOutcome {
    /// a succeeded outcome carrying `output`.
    pub fn succeeded(output: Value) -> Self {
        Self {
            status: WorkflowStatus::Succeeded,
            output,
        }
    }

    /// a failed outcome with a null output.
    pub fn failed() -> Self {
        Self {
            status: WorkflowStatus::Failed,
            output: Value::Null,
        }
    }
}
