#[allow(unused_imports)]
use super::*;

/// gate node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateState {
    pub gate_id: Option<Uuid>,
    #[serde(default)]
    pub deadline_unix: Option<i64>,
    pub poll_interval: i64,
}
