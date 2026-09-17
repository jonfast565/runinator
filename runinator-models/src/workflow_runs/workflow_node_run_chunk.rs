#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunChunk {
    pub id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub sequence: i64,
    pub stream: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}
