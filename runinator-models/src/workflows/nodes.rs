use super::*;
use crate::artifacts::{ArtifactKind, ArtifactPath, ArtifactRef};

/// classifies which terminal statuses a node is willing to retry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRetryClass {
    /// retry both `Failed` and `TimedOut` (the historical behavior).
    #[default]
    Any,
    /// retry `Failed` only; let a timeout fall straight through to its transition.
    Failure,
    /// retry `TimedOut` only; let an outright failure fall straight through.
    Timeout,
}

impl WorkflowRetryClass {
    /// true when a node run ending in `status` is eligible for retry under this policy.
    pub fn retryable(&self, status: WorkflowStatus) -> bool {
        match self {
            Self::Any => matches!(status, WorkflowStatus::Failed | WorkflowStatus::TimedOut),
            Self::Failure => status == WorkflowStatus::Failed,
            Self::Timeout => status == WorkflowStatus::TimedOut,
        }
    }
}

fn default_max_attempts() -> i64 {
    1
}

fn default_backoff_base_seconds() -> i64 {
    1
}

fn default_backoff_max_seconds() -> i64 {
    300
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSubflowType {
    #[default]
    Wait,
    FireAndForget,
}

mod workflow_node_id;
pub use workflow_node_id::WorkflowNodeId;

mod workflow_node_ref;
pub use workflow_node_ref::WorkflowNodeRef;

mod workflow_retry;
pub use workflow_retry::WorkflowRetry;

mod workflow_reentry;
pub use workflow_reentry::WorkflowReentry;

mod workflow_subflow;
pub use workflow_subflow::WorkflowSubflow;

mod workflow_transitions;
pub use workflow_transitions::WorkflowTransitions;

mod workflow_branch;
pub use workflow_branch::WorkflowBranch;

mod workflow_node;
pub use workflow_node::WorkflowNode;
