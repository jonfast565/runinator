#[allow(unused_imports)]
use super::*;

/// A run-level artifact declared by an output node, making it visible at workflow-run scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunArtifact {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub artifact_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}
