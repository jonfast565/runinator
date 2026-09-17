#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebounceState {
    pub deadline_unix: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_key: Option<String>,
}
