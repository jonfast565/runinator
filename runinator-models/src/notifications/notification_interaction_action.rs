#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationInteractionAction {
    pub id: String,
    pub label: String,
    #[serde(default = "default_interaction_input")]
    pub input: NotificationInteractionInput,
}
