#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceReceipt {
    pub id: Uuid,
    pub checkout: WorkspaceCheckout,
    pub snapshot: WorkspaceSnapshot,
}
