#[allow(unused_imports)]
use super::*;

/// `state.debug` configuration plus the primary cursor's runtime mirror.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugFrame {
    #[serde(flatten)]
    pub config: DebugConfig,
    #[serde(flatten)]
    pub runtime: DebugRuntime,
    #[serde(flatten)]
    pub extra: Map,
}
