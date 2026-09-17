#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalInteractionResponse {
    pub actor_subject: String,
    pub action_id: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub message_id: Option<String>,
}
