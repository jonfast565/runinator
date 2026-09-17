#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceCommit {
    pub checkout: WorkspaceCheckout,
    pub snapshot: WorkspaceSnapshot,
    pub receipt_id: Uuid,
}
