#[allow(unused_imports)]
use super::*;

/// A source-map range used by graph cursors, breakpoints, and execution history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSourceMapEntry {
    pub version: u32,
    pub instruction_start: usize,
    pub instruction_end: usize,
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_label: Option<String>,
    /// Whether an interrupt may suspend a thread positioned in this range. Compiled from the node
    /// kind's `GraphRole`, so the runtime never has to re-read the authoring definition.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interruptible: bool,
    /// Where control leaves this node on its normal path — the first instruction of its trailing
    /// exit sequence. An interrupt handler that answers `continue` sends the interrupted thread
    /// here; absent means the node has no single exit and `continue` degrades to `resume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_instruction_pointer: Option<usize>,
}

impl WorkflowSourceMapEntry {
    pub fn new(instruction_start: usize, instruction_end: usize, node_id: String) -> Self {
        Self {
            version: WORKFLOW_SOURCE_MAP_VERSION,
            instruction_start,
            instruction_end,
            node_id,
            edge_label: None,
            interruptible: false,
            exit_instruction_pointer: None,
        }
    }

    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_SOURCE_MAP_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_vm_version(
            WorkflowVmRecordKind::SourceMap,
            WORKFLOW_SOURCE_MAP_VERSION,
            self.version,
        )
    }
}
