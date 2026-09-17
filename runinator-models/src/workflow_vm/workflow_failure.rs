#[allow(unused_imports)]
use super::*;

/// A classified failure travelling through the VM. `Canceled` deliberately routes like `Failed`:
/// the graph has no cancel edge, and a run-level cancel retires continuations at the store instead
/// of resuming one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFailure {
    #[serde(default)]
    pub kind: WorkflowFailureKind,
    pub message: String,
}

impl WorkflowFailure {
    pub fn new(kind: WorkflowFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(WorkflowFailureKind::Failed, message)
    }
}

impl From<String> for WorkflowFailure {
    fn from(message: String) -> Self {
        Self::failed(message)
    }
}
