#[allow(unused_imports)]
use super::*;

/// `state.control` bookkeeping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlFrame {
    #[serde(default)]
    pub pause_requested: bool,
    #[serde(flatten)]
    pub extra: Map,
}
