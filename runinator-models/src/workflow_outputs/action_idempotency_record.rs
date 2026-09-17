#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionIdempotencyRecord {
    pub workflow_node_run_id: Uuid,
}
