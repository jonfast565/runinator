#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInterruptFrame {
    pub source: InterruptSource,
    pub interrupted_continuation_id: Uuid,
    pub resume_instruction_pointer: usize,
    /// First instruction of the interrupted node, for `restart`.
    #[serde(default)]
    pub node_start_instruction_pointer: usize,
    /// Where the interrupted node hands control on, for `continue`. Absent when the node has no
    /// single exit, which makes `continue` behave as `resume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_exit_instruction_pointer: Option<usize>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
    #[serde(default)]
    pub handled_at_instruction_pointers: Vec<usize>,
}
