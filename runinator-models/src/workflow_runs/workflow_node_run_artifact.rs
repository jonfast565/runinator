#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunArtifact {
    pub id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}
