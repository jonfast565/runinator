#[allow(unused_imports)]
use super::*;

/// subflow node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubflowState {
    pub subflow_run_id: Uuid,
    #[serde(default)]
    pub subflow_workflow_id: Uuid,
    #[serde(default)]
    pub run_name: Option<String>,
    #[serde(default)]
    pub reused: bool,
}
