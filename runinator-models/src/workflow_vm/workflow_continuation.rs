#[allow(unused_imports)]
use super::*;

/// Frozen workflow-machine state. One record represents one independently schedulable branch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowContinuation {
    /// Serialized continuation format version, checked independently from module bytecode.
    pub version: u32,
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub module_version: u32,
    pub instruction_pointer: usize,
    #[serde(default)]
    pub stack: Vec<Value>,
    #[serde(default)]
    pub locals: BTreeMap<String, Value>,
    /// Node entries observed since the last durable VM boundary. Persistence drains these into
    /// journal records atomically with the boundary, so inline nodes remain visible after reload.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_node_entries: Vec<String>,
    /// Structured execution state for nested control flow, invocation calls, compensation, and
    /// debugging. This deliberately has no graph cursor or node-run identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<WorkflowFrame>,
    /// Increments only after an effect is successfully requested; it is part of the idempotency
    /// identity for the next effect this branch emits.
    #[serde(default)]
    pub next_effect_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_effect_id: Option<Uuid>,
    pub status: WorkflowContinuationStatus,
    /// Run/debug operator hold, independent of an effect wait. A result settling while this is set
    /// leaves the continuation paused instead of making it runnable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub operator_paused: bool,
    /// An externally requested interrupt waiting for this thread to reach a safe point. It is
    /// consumed by the drive that decides about it — raised or refused — so nothing lingers to fire
    /// at an arbitrary later point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_interrupt: Option<WorkflowPendingInterrupt>,
    /// Compare-and-swap revision. Every durable transition increments this value.
    #[serde(default)]
    pub revision: u64,
}

impl WorkflowContinuation {
    pub fn start(workflow_run_id: Uuid, module_version: u32) -> Self {
        Self {
            version: WORKFLOW_CONTINUATION_VERSION,
            id: Uuid::now_v7(),
            workflow_run_id,
            module_version,
            instruction_pointer: 0,
            stack: Vec::new(),
            locals: BTreeMap::new(),
            pending_node_entries: Vec::new(),
            frames: Vec::new(),
            next_effect_sequence: 0,
            parent_id: None,
            fork_key: None,
            awaiting_effect_id: None,
            status: WorkflowContinuationStatus::Runnable,
            operator_paused: false,
            pending_interrupt: None,
            revision: 0,
        }
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_vm_version(
            WorkflowVmRecordKind::Continuation,
            WORKFLOW_CONTINUATION_VERSION,
            self.version,
        )
    }

    /// Whether this continuation is an interrupt handler running beside a frozen thread.
    ///
    /// A handler is excluded from run-terminal accounting: it can settle the interrupted node, but
    /// it can never decide the fate of the run.
    pub fn is_interrupt_handler(&self) -> bool {
        self.frames
            .iter()
            .any(|frame| matches!(frame, WorkflowFrame::Interrupt(_)))
    }
}

impl WorkflowContinuation {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_CONTINUATION_VERSION
    }
}
