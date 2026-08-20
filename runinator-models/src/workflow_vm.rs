//! Versioned, durable execution vocabulary for the workflow virtual machine.
//!
//! A workflow definition remains the authoring representation.  A run executes a compiled
//! [`WorkflowModule`], and every externally-observable operation is represented by one
//! [`WorkflowEffect`].  These types deliberately contain no store or broker details: the runtime
//! decides which transition comes next while its host durably records and delivers effects.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::workflows::{WorkflowCondition, WorkflowNodeKind};
use crate::{value::Value, workflows::WorkflowStatus};

/// The workflow bytecode version understood by this runtime.
pub const WORKFLOW_VM_VERSION: u32 = 1;
/// The effect broker envelope version. Kept separate so wire-only changes do not invalidate
/// already-snapshotted workflow bytecode.
pub const WORKFLOW_EFFECT_PROTOCOL_VERSION: u32 = 1;

/// An immutable compiled workflow snapshot attached to a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowModule {
    pub version: u32,
    pub instructions: Vec<WorkflowInstruction>,
    /// Maps executable locations back to the author-facing graph.
    #[serde(default)]
    pub source_map: Vec<WorkflowSourceMapEntry>,
}

impl WorkflowModule {
    pub fn new(instructions: Vec<WorkflowInstruction>) -> Self {
        Self {
            version: WORKFLOW_VM_VERSION,
            instructions,
            source_map: Vec::new(),
        }
    }

    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_VM_VERSION
    }

    /// Return the graph location containing an instruction pointer.
    pub fn graph_location(&self, ip: usize) -> Option<&WorkflowSourceMapEntry> {
        self.source_map
            .iter()
            .find(|entry| entry.instruction_start <= ip && ip < entry.instruction_end)
    }
}

/// A source-map range used by graph cursors, breakpoints, and execution history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSourceMapEntry {
    pub instruction_start: usize,
    pub instruction_end: usize,
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_label: Option<String>,
    /// Optional authoring-language byte range. JSON-authored graphs legitimately omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<WorkflowSourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSourceSpan {
    pub start: usize,
    pub end: usize,
}

/// The small workflow instruction set. Complex graph constructs lower to these control operations
/// plus typed effects rather than becoming host-side special cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WorkflowInstruction {
    /// Mark entry to an author-facing graph node. This is a no-op for evaluation but produces a
    /// stable source-map/journal boundary for cursors, breakpoints, and step-over.
    EnterNode {
        node_id: String,
    },
    Const {
        value: Value,
    },
    LoadLocal {
        name: String,
    },
    StoreLocal {
        name: String,
    },
    Pop,
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    /// Evaluate authoring conditions in declaration order and jump to the first match.
    Branch {
        branches: Vec<WorkflowVmBranch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<usize>,
    },
    /// A validated high-level selector (switch/toggle/percentage/loop/map/try). Keeping the
    /// selector kind explicit makes lowering exhaustive while the VM owns its deterministic
    /// evaluation semantics.
    Select {
        kind: WorkflowNodeKind,
        configuration: Value,
        targets: Vec<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<usize>,
    },
    /// A host-free graph operation whose entire input is frozen in the module.
    PureNode {
        kind: WorkflowNodeKind,
        configuration: Value,
    },
    /// Suspend this continuation until the named effect receives a terminal result.
    Effect {
        request: WorkflowEffectRequest,
    },
    /// Create one continuation per target. Each child has an independent effect sequence.
    Fork {
        targets: Vec<usize>,
        join_key: String,
    },
    /// Park this branch at a join until the host has all expected branch results.
    Join {
        join_key: String,
    },
    Return,
    Fail {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowVmBranch {
    pub condition: WorkflowCondition,
    pub target: usize,
}

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
    /// Compare-and-swap revision. Every durable transition increments this value.
    #[serde(default)]
    pub revision: u64,
}

impl WorkflowContinuation {
    pub fn start(workflow_run_id: Uuid, module_version: u32) -> Self {
        Self {
            version: WORKFLOW_VM_VERSION,
            id: Uuid::now_v7(),
            workflow_run_id,
            module_version,
            instruction_pointer: 0,
            stack: Vec::new(),
            locals: BTreeMap::new(),
            next_effect_sequence: 0,
            parent_id: None,
            fork_key: None,
            awaiting_effect_id: None,
            status: WorkflowContinuationStatus::Runnable,
            revision: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowContinuationStatus {
    Runnable,
    Waiting,
    Joined,
    Succeeded,
    Failed,
    Canceled,
}

impl WorkflowContinuationStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }
}

/// A request emitted by the VM. It is converted to an effect record by the durable host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowEffectRequest {
    Action {
        provider: String,
        function: String,
        input: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_seconds: Option<i64>,
    },
    Timer {
        due_at: i64,
    },
    TimerDelay {
        seconds: i64,
    },
    Approval {
        prompt: Value,
        expires_at: Option<i64>,
    },
    Signal {
        key: String,
        filter: Option<Value>,
    },
    Input {
        schema: Value,
    },
    ChildRun {
        workflow_id: Uuid,
        input: Value,
    },
    MutexAcquire {
        key: String,
    },
    /// Infrastructure-owned durable effects use a stable kind name and frozen payload. Provider
    /// workers must reject this variant; the engine/web-service coordination host owns it.
    Coordination {
        kind: String,
        input: Value,
    },
}

/// The canonical durable receipt for a yielded effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowEffect {
    pub version: u32,
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub continuation_id: Uuid,
    pub sequence: u64,
    pub attempt: u32,
    pub request: WorkflowEffectRequest,
    pub status: WorkflowEffectStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Unix seconds. Immutable receipt creation time, independent of broker publication.
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}

/// One immutable execution-history record. `sequence` is per workflow run and is allocated by the
/// transaction that mutates the continuation/effect state, making UI history stable across retries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowJournalRecord {
    pub version: u32,
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<Uuid>,
    pub entry: WorkflowJournalEntry,
    pub created_at: i64,
}

impl WorkflowEffect {
    pub fn idempotency_key(&self) -> String {
        format!(
            "workflow-effect:{}:{}:{}",
            self.continuation_id, self.sequence, self.attempt
        )
    }

    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_EFFECT_PROTOCOL_VERSION
    }
}

impl WorkflowContinuation {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_VM_VERSION
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEffectStatus {
    Requested,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Canceled,
}

impl WorkflowEffectStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::TimedOut | Self::Canceled
        )
    }

    pub fn workflow_status(self) -> WorkflowStatus {
        match self {
            Self::Requested | Self::Running => WorkflowStatus::Waiting,
            Self::Succeeded => WorkflowStatus::Succeeded,
            Self::Failed => WorkflowStatus::Failed,
            Self::TimedOut => WorkflowStatus::TimedOut,
            Self::Canceled => WorkflowStatus::Canceled,
        }
    }
}

/// An append-only execution-history event. This replaces node-run history without making the UI
/// infer transitions from mutable continuation rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowJournalEntry {
    Entered {
        continuation_id: Uuid,
        instruction_pointer: usize,
    },
    Transitioned {
        continuation_id: Uuid,
        instruction_pointer: usize,
    },
    Forked {
        continuation_id: Uuid,
        children: Vec<Uuid>,
        join_key: String,
    },
    EffectRequested {
        effect_id: Uuid,
    },
    EffectSettled {
        effect_id: Uuid,
        status: WorkflowEffectStatus,
    },
    Completed {
        continuation_id: Uuid,
        value: Value,
    },
    Failed {
        continuation_id: Uuid,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_map_keeps_graph_cursor_identity() {
        let mut module = WorkflowModule::new(vec![WorkflowInstruction::Return]);
        module.source_map.push(WorkflowSourceMapEntry {
            instruction_start: 0,
            instruction_end: 1,
            node_id: "publish".into(),
            edge_label: Some("next".into()),
            source_span: None,
        });

        assert_eq!(
            module.graph_location(0).map(|entry| entry.node_id.as_str()),
            Some("publish")
        );
        assert!(module.graph_location(1).is_none());
    }

    #[test]
    fn effect_key_is_stable_for_a_continuation_sequence_and_attempt() {
        let effect = WorkflowEffect {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            id: Uuid::now_v7(),
            workflow_run_id: Uuid::now_v7(),
            continuation_id: Uuid::nil(),
            sequence: 7,
            attempt: 2,
            request: WorkflowEffectRequest::Timer { due_at: 1 },
            status: WorkflowEffectStatus::Requested,
            result: None,
            message: None,
            created_at: 1,
            updated_at: 1,
            finished_at: None,
        };
        assert_eq!(
            effect.idempotency_key(),
            "workflow-effect:00000000-0000-0000-0000-000000000000:7:2"
        );
    }
}
