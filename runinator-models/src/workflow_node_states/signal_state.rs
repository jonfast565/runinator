#[allow(unused_imports)]
use super::*;

/// signal node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalState {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_key: Option<String>,
}
