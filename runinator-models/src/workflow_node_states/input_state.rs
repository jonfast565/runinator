#[allow(unused_imports)]
use super::*;

/// input node state while it waits for a user response in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputState {
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<Uuid>,
}
