#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowForkFrame {
    pub fork_key: String,
    pub parent_id: Uuid,
    pub branch_index: u64,
}
