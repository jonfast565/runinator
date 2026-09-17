#[allow(unused_imports)]
use super::*;

/// `state.compensation` saga-rollback bookkeeping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompensationFrame {
    #[serde(default)]
    pub remaining: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<Uuid>,
}
