#[allow(unused_imports)]
use super::*;

/// A program-authored lifecycle boundary embedded in a PTY stream. The input bytes themselves
/// remain ephemeral; this small record is safe to retain with the effect's durable output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInteraction {
    pub sequence: u64,
    pub request_id: String,
    pub state: TerminalInteractionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}
