#[allow(unused_imports)]
use super::*;

/// Engine-resolved workspace input supplied only to the assigned worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceExecution {
    pub key: String,
    pub checkout: WorkspaceCheckout,
    pub snapshot: Option<WorkspaceSnapshot>,
    pub results: BTreeMap<String, Value>,
}
