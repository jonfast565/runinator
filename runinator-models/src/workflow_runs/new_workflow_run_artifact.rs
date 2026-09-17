#[allow(unused_imports)]
use super::*;

/// Input for promoting a node artifact to a run-level artifact via an output node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWorkflowRunArtifact {
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub artifact_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
}
