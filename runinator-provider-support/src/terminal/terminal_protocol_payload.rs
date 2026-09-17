#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct TerminalProtocolPayload {
    pub(super) version: u8,
    pub(super) event: String,
    pub(super) request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) prompt: Option<String>,
}
