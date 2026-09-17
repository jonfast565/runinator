#[allow(unused_imports)]
use super::*;

/// `state.subflow_parent`: the parent run and node a child run reports back to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubflowParent {
    pub run_id: Uuid,
    pub node_id: String,
}
