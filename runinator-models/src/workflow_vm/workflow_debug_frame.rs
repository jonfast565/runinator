#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDebugFrame {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub step_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_to_node_id: Option<String>,
    /// A failure parked before structured error routing. Resuming consumes it exactly once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_failure: Option<WorkflowFailure>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pause_on_failure: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_output: Option<Value>,
    /// Speculative continuations cannot settle durable effects unless explicitly armed.
    #[serde(default)]
    pub speculative: bool,
}
