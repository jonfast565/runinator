#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub approval_type: String,
    pub prompt: String,
    pub status: String,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub metadata: Value,
}
