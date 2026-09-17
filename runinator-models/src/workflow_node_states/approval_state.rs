#[allow(unused_imports)]
use super::*;

/// approval node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalState {
    pub approval: Value,
    pub approval_id: Option<Uuid>,
}
