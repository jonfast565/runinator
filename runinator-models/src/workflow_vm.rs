//! Versioned, durable execution vocabulary for the workflow virtual machine.
//!
//! A workflow definition remains the authoring representation.  A run executes a compiled
//! [`WorkflowModule`], and every externally-observable operation is represented by one
//! [`WorkflowEffect`].  These types deliberately contain no store or broker details: the runtime
//! decides which transition comes next while its host durably records and delivers effects.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::functions::FunctionBinding;
use crate::interrupt::{InterruptMode, InterruptSource};
use crate::invocation::InvocationModule;
use crate::orchestration::GateKind;
use crate::workflows::{WorkflowCondition, WorkflowNodeKind, WorkflowRetry};
use crate::{value::Value, workflows::WorkflowStatus};

/// The workflow bytecode version understood by this runtime.
pub const WORKFLOW_VM_VERSION: u32 = 1;
/// The serialized continuation version. It intentionally evolves independently of bytecode.
pub const WORKFLOW_CONTINUATION_VERSION: u32 = 1;
/// The source-map format version embedded in a workflow module.
pub const WORKFLOW_SOURCE_MAP_VERSION: u32 = 1;
/// The append-only journal entry format version.
pub const WORKFLOW_JOURNAL_VERSION: u32 = 1;
/// The effect broker envelope version. Kept separate so wire-only changes do not invalidate
/// already-snapshotted workflow bytecode.
pub const WORKFLOW_EFFECT_PROTOCOL_VERSION: u32 = 1;

/// Wall-clock budget for a provider action that declares no `timeout_seconds`.
///
/// The worker enforces this in process, and the engine arms its deadline backstop from the same
/// value. Both sides read this constant so an action cannot end up with two different deadlines.
pub const DEFAULT_ACTION_TIMEOUT_SECONDS: i64 = 60;

/// The record whose version a compatibility check rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowVmRecordKind {
    Module,
    Continuation,
    SourceMap,
    Effect,
    Journal,
}

impl std::fmt::Display for WorkflowVmRecordKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Module => "module",
            Self::Continuation => "continuation",
            Self::SourceMap => "source map",
            Self::Effect => "effect",
            Self::Journal => "journal record",
        })
    }
}

/// A persisted or wire record was produced by a VM revision this process does not understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedWorkflowVmVersion {
    pub record: WorkflowVmRecordKind,
    pub expected: u32,
    pub actual: u32,
}

impl std::fmt::Display for UnsupportedWorkflowVmVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "unsupported workflow VM {} version {}; expected {}",
            self.record, self.actual, self.expected
        )
    }
}

impl std::error::Error for UnsupportedWorkflowVmVersion {}

fn ensure_vm_version(
    record: WorkflowVmRecordKind,
    expected: u32,
    actual: u32,
) -> Result<(), UnsupportedWorkflowVmVersion> {
    if actual == expected {
        Ok(())
    } else {
        Err(UnsupportedWorkflowVmVersion {
            record,
            expected,
            actual,
        })
    }
}

/// Reject an incompatible effect-protocol envelope before handling its payload.
pub fn ensure_effect_protocol_version(actual: u32) -> Result<(), UnsupportedWorkflowVmVersion> {
    if actual == WORKFLOW_EFFECT_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(UnsupportedWorkflowVmVersion {
            record: WorkflowVmRecordKind::Effect,
            expected: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            actual,
        })
    }
}

/// An immutable compiled workflow snapshot attached to a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowModule {
    pub version: u32,
    pub instructions: Vec<WorkflowInstruction>,
    /// Maps executable locations back to the author-facing graph.
    #[serde(default)]
    pub source_map: Vec<WorkflowSourceMapEntry>,
    /// Compiled interrupt handler entries. Frozen into the module rather than looked up in mutable
    /// workflow metadata, so a run in flight keeps the handlers it started with. Timer handlers
    /// are keyed by their distinct `timer_id`, allowing several periodic handlers in one workflow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interrupt_handlers: Vec<WorkflowVmInterruptHandler>,
}

impl WorkflowModule {
    pub fn new(instructions: Vec<WorkflowInstruction>) -> Self {
        Self {
            version: WORKFLOW_VM_VERSION,
            instructions,
            source_map: Vec::new(),
            interrupt_handlers: Vec::new(),
        }
    }

    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_VM_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_vm_version(
            WorkflowVmRecordKind::Module,
            WORKFLOW_VM_VERSION,
            self.version,
        )?;
        for entry in &self.source_map {
            entry.ensure_supported()?;
        }
        Ok(())
    }

    /// Return the graph location containing an instruction pointer.
    ///
    /// The compiler lays blocks out consecutively, so the ranges are sorted and disjoint and this
    /// can bisect rather than scan — it is called once per drive and once per rendered cursor.
    /// `source_map_is_ordered` pins the invariant this relies on.
    pub fn graph_location(&self, ip: usize) -> Option<&WorkflowSourceMapEntry> {
        let index = self
            .source_map
            .binary_search_by(|entry| {
                if entry.instruction_end <= ip {
                    std::cmp::Ordering::Less
                } else if entry.instruction_start > ip {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .ok()?;
        self.source_map.get(index)
    }

    /// Whether the source map is sorted and non-overlapping, which is what makes
    /// [`Self::graph_location`] a bisection. Compiled modules always satisfy it.
    pub fn source_map_is_ordered(&self) -> bool {
        self.source_map.windows(2).all(|pair| {
            pair[0].instruction_start <= pair[0].instruction_end
                && pair[0].instruction_end <= pair[1].instruction_start
        })
    }

    /// The compiled handler for one non-timer source, if the workflow declared it.
    pub fn interrupt_handler(
        &self,
        source: InterruptSource,
    ) -> Option<&WorkflowVmInterruptHandler> {
        self.interrupt_handlers
            .iter()
            .find(|handler| handler.source == source)
    }

    /// Select the frozen handler for a pending request. Timer requests carry the declaration's
    /// stable handler id in their payload, while the other sources remain one-per-source.
    pub fn interrupt_handler_for(
        &self,
        source: InterruptSource,
        payload: &Value,
    ) -> Option<&WorkflowVmInterruptHandler> {
        if source != InterruptSource::Timer {
            return self.interrupt_handler(source);
        }
        let timer_id = payload.get("timer_id").and_then(Value::as_str)?;
        self.interrupt_handlers.iter().find(|handler| {
            handler.source == InterruptSource::Timer
                && handler.timer_id.as_deref() == Some(timer_id)
        })
    }
}

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
    /// Evaluate compiled compute code and push its result. The invocation continuation, when the
    /// program yields, lives in [`WorkflowFrame::Invocation`], never in node-run state.
    Evaluate {
        module: InvocationModule,
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
    /// Allocate a durable loop frame from a frozen item collection. `body` is entered for each
    /// item; `exit` is entered after the final item or the iteration limit.
    BeginLoop {
        loop_key: String,
        body: usize,
        exit: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_iterations: Option<u64>,
    },
    /// Advance the loop identified by `loop_key`, recording the stack's top value as the current
    /// item result when present.
    NextLoop {
        loop_key: String,
    },
    /// Guard a graph re-entry point. The visit count is persisted in a re-entry frame, not
    /// inferred from historic node runs.
    Reenter {
        reentry_key: String,
        target: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exhausted: Option<usize>,
        max_visits: u64,
    },
    /// Enter a structured try region. `catch` and `finally` are explicit control-flow targets so
    /// failure does not depend on host-side graph traversal.
    BeginTry {
        try_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        catch: Option<usize>,
        /// Graph `on_timeout` edge, preferred over `catch` for a timed-out step.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_timeout: Option<usize>,
        /// Graph `on_reject` edge, preferred over `catch` for a rejected step.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_reject: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finally: Option<usize>,
    },
    EndTry {
        try_key: String,
    },
    /// Register an already-successful effect's compensator. The compensator itself is emitted as
    /// a normal effect while [`WorkflowFrame::Compensation`] tracks the unwind.
    RegisterCompensation {
        compensation_key: String,
        request: WorkflowEffectRequest,
    },
    BeginCompensation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume: Option<usize>,
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
        /// Number of branch arrivals that belong to this join visit. This is compiled from the
        /// immutable graph rather than inferred from live cursor rows.
        expected: u64,
        #[serde(default)]
        mode: WorkflowBranchPolicy,
    },
    /// Fork a race. The first terminal arrival wins; the persisted race frame records the winner
    /// and makes loser cancellation deterministic after restart.
    Race {
        targets: Vec<usize>,
        race_key: String,
        #[serde(default = "WorkflowBranchPolicy::first_success")]
        winner: WorkflowBranchPolicy,
    },
    /// Start a bounded map. Parent scheduling and each child item's binding are continuation
    /// frames, which permits a map to resume without child-run records.
    BeginMap {
        map_key: String,
        body: usize,
        exit: usize,
        concurrency: u64,
    },
    /// An interrupt safe-point. The host may create a handler continuation from the supplied
    /// target and freeze the interrupted continuation in an interrupt frame.
    CheckInterrupt {
        handlers: Vec<WorkflowVmInterruptHandler>,
    },
    /// Complete an interrupt handler and apply its declared disposition to the frozen branch.
    ResumeInterrupt {
        mode: InterruptMode,
    },
    /// A debugger boundary independent of `EnterNode`; this lets breakpoints target source-map
    /// locations inside a compiled node.
    DebugBoundary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// Set the workflow's terminal output without ending the current continuation. The artifact
    /// sources are compiled programs so the VM never has to rediscover expressions in node JSON.
    SetOutput {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_type: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        artifacts: Vec<WorkflowOutputArtifact>,
    },
    Return,
    Fail {
        message: String,
    },
}

/// The deterministic completion policy shared by joins and races.  It deliberately lives in the
/// VM record crate instead of the graph parser: persisted continuations must remain interpretable
/// after the authoring definition is no longer available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowBranchPolicy {
    #[default]
    All,
    Any,
    FirstSuccess,
}

impl WorkflowBranchPolicy {
    pub const fn first_success() -> Self {
        Self::FirstSuccess
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowVmBranch {
    pub condition: WorkflowCondition,
    pub target: usize,
}

/// One run-level artifact declaration attached to an output instruction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowOutputArtifact {
    pub name: String,
    pub source: InvocationModule,
}

/// An interrupt asked for out of band, recorded on the thread it targets until that thread reaches
/// a safe point. `External` and `OrphanSignal` arrive this way; the sources the VM detects for
/// itself never take this route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowPendingInterrupt {
    pub id: Uuid,
    pub source: InterruptSource,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}

/// What a finished handler decided for the thread it suspended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowInterruptOutcome {
    /// Make the interrupted thread runnable again at this location.
    Resume { instruction_pointer: usize },
    /// Settle the interrupted node failed and let the main flow's own routing decide. A handler
    /// can never fail the run directly; this is the strongest thing it can say.
    Fail { message: String },
}

/// One compiled interrupt handler target. The source is part of bytecode rather than a lookup in
/// mutable workflow metadata, so a run remains reproducible after its definition changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowVmInterruptHandler {
    pub source: InterruptSource,
    pub target: usize,
    /// Stable identity of a periodic timer declaration. Only set for [`InterruptSource::Timer`].
    /// It is distinct even when two schedules use the same handler region; the timer relay carries
    /// it in its pending-interrupt payload, so multiple timers are never ambiguous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_id: Option<String>,
    /// The frozen period for a periodic timer handler, in seconds. Only set for timer handlers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_seconds: Option<i64>,
}

/// Durable state scoped to one continuation. Frames replace the graph reducer's cursor, node-run,
/// and invocation-call bookkeeping; every value needed to resume a branch is serializable here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowFrame {
    Loop(WorkflowLoopFrame),
    Reentry(WorkflowReentryFrame),
    Try(WorkflowTryFrame),
    Map(WorkflowMapFrame),
    Fork(WorkflowForkFrame),
    Join(WorkflowJoinFrame),
    Race(WorkflowRaceFrame),
    Interrupt(WorkflowInterruptFrame),
    Compensation(WorkflowCompensationFrame),
    Invocation(WorkflowInvocationFrame),
    Debug(WorkflowDebugFrame),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowLoopFrame {
    pub loop_key: String,
    pub body: usize,
    pub exit: usize,
    #[serde(default)]
    pub index: u64,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub results: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowReentryFrame {
    pub reentry_key: String,
    #[serde(default)]
    pub visits: u64,
    pub max_visits: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTryPhase {
    Body,
    Catch,
    Finally,
}

/// Why a step did not succeed. The graph distinguishes `on_failure`, `on_timeout`, and
/// `on_reject`, so a bare message is not enough to pick the edge a run should take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowFailureKind {
    #[default]
    Failed,
    TimedOut,
    Rejected,
    Canceled,
}

/// A classified failure travelling through the VM. `Canceled` deliberately routes like `Failed`:
/// the graph has no cancel edge, and a run-level cancel retires continuations at the store instead
/// of resuming one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFailure {
    #[serde(default)]
    pub kind: WorkflowFailureKind,
    pub message: String,
}

impl WorkflowFailure {
    pub fn new(kind: WorkflowFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(WorkflowFailureKind::Failed, message)
    }
}

impl From<String> for WorkflowFailure {
    fn from(message: String) -> Self {
        Self::failed(message)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowTryFrame {
    pub try_key: String,
    pub phase: WorkflowTryPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catch: Option<usize>,
    /// Preferred catch target for a timed-out step (`on_timeout`), falling back to `catch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_timeout: Option<usize>,
    /// Preferred catch target for a rejected step (`on_reject`), falling back to `catch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_reject: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finally: Option<usize>,
    /// Captured before `finally` runs, then re-applied after it completes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowMapFrame {
    pub map_key: String,
    pub body: usize,
    pub exit: usize,
    pub concurrency: u64,
    #[serde(default)]
    pub next_index: u64,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub results: Vec<WorkflowIndexedValue>,
    /// The item carried by a child continuation. Its index is enough to order the parent result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_index: Option<u64>,
}

/// A result labelled with a fork or map index. A vector is intentional: JSON object keys are
/// strings, while this representation preserves a numeric index and a deterministic order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowIndexedValue {
    pub index: u64,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowForkFrame {
    pub fork_key: String,
    pub parent_id: Uuid,
    pub branch_index: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowJoinFrame {
    pub join_key: String,
    pub expected: u64,
    #[serde(default)]
    pub mode: WorkflowBranchPolicy,
    #[serde(default)]
    pub arrivals: Vec<WorkflowIndexedValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRaceFrame {
    pub race_key: String,
    pub expected: u64,
    #[serde(default = "WorkflowBranchPolicy::first_success")]
    pub winner_policy: WorkflowBranchPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_value: Option<Value>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowCompensationFrame {
    #[serde(default)]
    pub pending: Vec<WorkflowEffectRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<WorkflowEffectRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInvocationFrame {
    pub module: InvocationModule,
    pub continuation: crate::invocation::InvocationContinuation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDebugFrame {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub step_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_output: Option<Value>,
    /// Speculative continuations cannot settle durable effects unless explicitly armed.
    #[serde(default)]
    pub speculative: bool,
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

/// The graph-facing view of a durable continuation.  Execution identity stays the continuation
/// id; the node id is derived only for rendering from the frozen module source map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowVmCursor {
    pub continuation_id: Uuid,
    pub instruction_pointer: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_label: Option<String>,
    pub status: WorkflowContinuationStatus,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowContinuationStatus {
    Runnable,
    /// Frozen behind an interrupt handler running beside it. Never claimed by a scheduler; the
    /// handler's `resume` is what makes it runnable again.
    Suspended,
    /// Parked by an operator/debugger. Unlike `Waiting`, this continuation is not awaiting an
    /// effect result and can be made runnable again without changing effect state.
    Paused,
    Waiting,
    Joined,
    Succeeded,
    Failed,
    Canceled,
}

/// A durable, deduplicated piece of output produced while an effect is executing. Output events
/// are addressed by effect/continuation identity and never by a graph node-run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowEffectOutputEvent {
    pub event_id: Uuid,
    pub effect_id: Uuid,
    pub workflow_run_id: Uuid,
    pub continuation_id: Uuid,
    pub attempt: u32,
    pub output: WorkflowEffectOutput,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowEffectOutput {
    Chunk {
        stream: String,
        content: String,
    },
    Artifact {
        artifact: Value,
    },
    TerminalInteraction {
        interaction: crate::runs::TerminalInteraction,
    },
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
        /// The node retry contract is part of the immutable request.  It must not be recovered
        /// from a mutable workflow definition when a delivery is retried after a deploy.
        #[serde(default)]
        retry: WorkflowRetry,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
        /// Worker-routing constraints frozen with the request rather than looked up from the
        /// authoring graph by a dispatcher.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        required_labels: BTreeMap<String, String>,
        /// Resolved workspace token retained with the effect receipt and dispatch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_affinity: Option<Value>,
        /// A still-unresolved key expression. The VM host evaluates and records it before the
        /// effect is delivered, so redelivery cannot observe a changed workflow definition.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idempotency_key: Option<Value>,
        /// Packaged functions are addressed by their immutable binding, not by a provider catalog
        /// lookup that may have changed between compile and delivery.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        function_binding: Option<FunctionBinding>,
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
    Gate {
        kind: GateKind,
        condition: WorkflowCondition,
        poll_interval_seconds: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deadline_seconds: Option<i64>,
        #[serde(default)]
        continue_on_timeout: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Value::is_null")]
        metadata: Value,
    },
    Signal {
        key: String,
        filter: Option<Value>,
    },
    Input {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        schema: Value,
    },
    /// Suspend until an event matching this frozen subscription arrives.  This is intentionally
    /// an effect rather than a polling instruction: the coordination host owns the subscription.
    EventWait {
        event_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_events: Option<u64>,
    },
    ChildRun {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_name: Option<String>,
        /// Exact immutable target selected by an authored revision pin. Both this and its digest
        /// are carried in the durable effect receipt so a delayed dispatch cannot observe a later
        /// workflow head.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_revision: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_revision_digest: Option<String>,
        input: Value,
        #[serde(default)]
        wait: bool,
        #[serde(default)]
        reuse_open_run: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_name: Option<Value>,
    },
    /// Wait for existing child or peer workflow runs without creating another one.
    AwaitRun {
        workflow: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<Value>,
        #[serde(default = "default_await_run_mode")]
        mode: String,
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

fn default_await_run_mode() -> String {
    "all".to_string()
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
    /// Source-map projection populated by the operator API. It is not stored with the effect
    /// receipt, because the pinned module is the source of truth for that relationship.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub request: WorkflowEffectRequest,
    pub status: WorkflowEffectStatus,
    /// Replica currently executing this attempt, set when a host claims the delivery and cleared
    /// when the effect settles. This is the VM's executor lease: it replaces the node-run executor
    /// columns, so replica load and dead-worker recovery read effects rather than node runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_executor_replica_id: Option<Uuid>,
    /// Last replica to have claimed this effect, retained after settlement for attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executor_replica_id: Option<Uuid>,
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

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_effect_protocol_version(self.version)
    }
}

impl WorkflowContinuation {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_CONTINUATION_VERSION
    }
}

impl WorkflowJournalRecord {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_JOURNAL_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_vm_version(
            WorkflowVmRecordKind::Journal,
            WORKFLOW_JOURNAL_VERSION,
            self.version,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEffectStatus {
    Requested,
    Running,
    InputRequired,
    Succeeded,
    Failed,
    /// An external decision declined the step (an approval or input rejection). Distinct from
    /// `Failed` only so the graph can take its `on_reject` edge instead of `on_failure`.
    Rejected,
    TimedOut,
    Canceled,
}

impl WorkflowEffectStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Rejected | Self::TimedOut | Self::Canceled
        )
    }

    pub fn workflow_status(self) -> WorkflowStatus {
        match self {
            Self::Requested | Self::Running => WorkflowStatus::Waiting,
            Self::InputRequired => WorkflowStatus::InputRequired,
            Self::Succeeded => WorkflowStatus::Succeeded,
            Self::Failed | Self::Rejected => WorkflowStatus::Failed,
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
    /// An author-facing graph node began executing. Unlike an effect receipt, this also covers
    /// pure nodes such as Config, Transform, and Assert.
    NodeEntered {
        continuation_id: Uuid,
        node_id: String,
    },
    Forked {
        continuation_id: Uuid,
        children: Vec<Uuid>,
        join_key: String,
    },
    EffectRequested {
        effect_id: Uuid,
        /// The yielding opcode. Together with the run's frozen module this identifies the graph
        /// node that requested the effect, even after its continuation moves on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instruction_pointer: Option<usize>,
    },
    EffectSettled {
        effect_id: Uuid,
        status: WorkflowEffectStatus,
    },
    /// A failed effect was re-armed by its node's retry policy rather than settled. The attempt is
    /// the one about to run, so a reader can tell attempt 2 of 3 from the terminal that follows it.
    EffectRetryScheduled {
        effect_id: Uuid,
        attempt: u32,
        available_at: i64,
    },
    Completed {
        continuation_id: Uuid,
        value: Value,
    },
    Failed {
        continuation_id: Uuid,
        message: String,
        /// The last graph node entered by this continuation, if failure happened during inline
        /// evaluation before an effect could be requested.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node_id: Option<String>,
    },
    /// A thread was frozen and a handler continuation started beside it.
    Interrupted {
        continuation_id: Uuid,
        handler_continuation_id: Uuid,
        source: InterruptSource,
    },
    /// A handler finished and handed control back.
    InterruptResolved {
        continuation_id: Uuid,
        handler_continuation_id: Uuid,
        outcome: WorkflowInterruptOutcome,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_map_keeps_graph_cursor_identity() {
        let mut module = WorkflowModule::new(vec![WorkflowInstruction::Return]);
        module.source_map.push(WorkflowSourceMapEntry {
            version: WORKFLOW_SOURCE_MAP_VERSION,
            instruction_start: 0,
            instruction_end: 1,
            node_id: "publish".into(),
            edge_label: Some("next".into()),
            interruptible: true,
            exit_instruction_pointer: Some(1),
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
            node_id: None,
            request: WorkflowEffectRequest::Timer { due_at: 1 },
            status: WorkflowEffectStatus::Requested,
            current_executor_replica_id: None,
            last_executor_replica_id: None,
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

    #[test]
    fn vm_records_have_pinned_json_shapes() {
        let module = WorkflowModule {
            version: WORKFLOW_VM_VERSION,
            instructions: vec![WorkflowInstruction::Return],
            source_map: vec![WorkflowSourceMapEntry::new(0, 1, "done".into())],
            interrupt_handlers: Vec::new(),
        };
        let continuation = WorkflowContinuation {
            id: Uuid::nil(),
            workflow_run_id: Uuid::nil(),
            module_version: WORKFLOW_VM_VERSION,
            ..WorkflowContinuation::start(Uuid::nil(), WORKFLOW_VM_VERSION)
        };
        let effect = WorkflowEffect {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            id: Uuid::nil(),
            workflow_run_id: Uuid::nil(),
            continuation_id: Uuid::nil(),
            sequence: 0,
            attempt: 0,
            node_id: None,
            request: WorkflowEffectRequest::Timer { due_at: 1 },
            status: WorkflowEffectStatus::Requested,
            current_executor_replica_id: None,
            last_executor_replica_id: None,
            result: None,
            message: None,
            created_at: 0,
            updated_at: 0,
            finished_at: None,
        };
        let journal = WorkflowJournalRecord {
            version: WORKFLOW_JOURNAL_VERSION,
            id: Uuid::nil(),
            workflow_run_id: Uuid::nil(),
            sequence: 0,
            continuation_id: Some(Uuid::nil()),
            effect_id: None,
            entry: WorkflowJournalEntry::Entered {
                continuation_id: Uuid::nil(),
                instruction_pointer: 0,
            },
            created_at: 0,
        };

        assert_eq!(
            serde_json::to_string(&module).unwrap(),
            r#"{"version":1,"instructions":[{"op":"return"}],"source_map":[{"version":1,"instruction_start":0,"instruction_end":1,"node_id":"done"}]}"#
        );
        assert_eq!(
            serde_json::to_string(&continuation).unwrap(),
            r#"{"version":1,"id":"00000000-0000-0000-0000-000000000000","workflow_run_id":"00000000-0000-0000-0000-000000000000","module_version":1,"instruction_pointer":0,"stack":[],"locals":{},"next_effect_sequence":0,"status":"runnable","revision":0}"#
        );
        assert_eq!(
            serde_json::to_string(&effect).unwrap(),
            r#"{"version":1,"id":"00000000-0000-0000-0000-000000000000","workflow_run_id":"00000000-0000-0000-0000-000000000000","continuation_id":"00000000-0000-0000-0000-000000000000","sequence":0,"attempt":0,"request":{"type":"timer","due_at":1},"status":"requested","created_at":0,"updated_at":0}"#
        );
        assert_eq!(
            serde_json::to_string(&journal).unwrap(),
            r#"{"version":1,"id":"00000000-0000-0000-0000-000000000000","workflow_run_id":"00000000-0000-0000-0000-000000000000","sequence":0,"continuation_id":"00000000-0000-0000-0000-000000000000","entry":{"type":"entered","continuation_id":"00000000-0000-0000-0000-000000000000","instruction_pointer":0},"created_at":0}"#
        );
    }

    #[test]
    fn incompatible_record_versions_are_explicit_errors() {
        let module = WorkflowModule {
            version: WORKFLOW_VM_VERSION + 1,
            instructions: vec![],
            source_map: vec![],
            interrupt_handlers: vec![],
        };
        let source_map = WorkflowSourceMapEntry {
            version: WORKFLOW_SOURCE_MAP_VERSION + 1,
            instruction_start: 0,
            instruction_end: 1,
            node_id: "node".into(),
            edge_label: None,
            interruptible: false,
            exit_instruction_pointer: None,
        };
        assert_eq!(
            module.ensure_supported().unwrap_err().record,
            WorkflowVmRecordKind::Module
        );
        assert_eq!(
            source_map.ensure_supported().unwrap_err().record,
            WorkflowVmRecordKind::SourceMap
        );
        assert_eq!(
            ensure_effect_protocol_version(WORKFLOW_EFFECT_PROTOCOL_VERSION + 1)
                .unwrap_err()
                .record,
            WorkflowVmRecordKind::Effect
        );
    }

    #[test]
    fn unknown_opcodes_are_decode_errors() {
        let error =
            serde_json::from_str::<WorkflowInstruction>(r#"{"op":"from_future"}"#).unwrap_err();
        assert!(error.to_string().contains("from_future"));
    }

    #[test]
    fn continuation_frames_capture_every_structured_runtime_state() {
        let mut continuation = WorkflowContinuation::start(Uuid::nil(), WORKFLOW_VM_VERSION);
        continuation.frames = vec![
            WorkflowFrame::Loop(WorkflowLoopFrame {
                loop_key: "loop".into(),
                body: 1,
                exit: 2,
                index: 1,
                items: vec![Value::from("item")],
                results: vec![Value::from("result")],
                max_iterations: Some(3),
            }),
            WorkflowFrame::Reentry(WorkflowReentryFrame {
                reentry_key: "retry".into(),
                visits: 2,
                max_visits: 3,
            }),
            WorkflowFrame::Try(WorkflowTryFrame {
                try_key: "try".into(),
                phase: WorkflowTryPhase::Finally,
                catch: Some(3),
                on_timeout: None,
                on_reject: None,
                finally: Some(4),
                pending_failure: Some("original failure".into()),
            }),
            WorkflowFrame::Map(WorkflowMapFrame {
                map_key: "map".into(),
                body: 5,
                exit: 6,
                concurrency: 2,
                next_index: 1,
                items: vec![Value::from("item")],
                results: vec![WorkflowIndexedValue {
                    index: 0,
                    value: Value::from("result"),
                }],
                item: Some(Value::from("item")),
                item_index: Some(0),
            }),
            WorkflowFrame::Fork(WorkflowForkFrame {
                fork_key: "fork".into(),
                parent_id: Uuid::nil(),
                branch_index: 0,
            }),
            WorkflowFrame::Join(WorkflowJoinFrame {
                join_key: "join".into(),
                expected: 2,
                mode: WorkflowBranchPolicy::All,
                arrivals: vec![WorkflowIndexedValue {
                    index: 0,
                    value: Value::from("left"),
                }],
            }),
            WorkflowFrame::Race(WorkflowRaceFrame {
                race_key: "race".into(),
                expected: 2,
                winner_policy: WorkflowBranchPolicy::first_success(),
                winner: Some(Uuid::nil()),
                winner_value: Some(Value::from("winner")),
            }),
            WorkflowFrame::Interrupt(WorkflowInterruptFrame {
                node_start_instruction_pointer: 0,
                node_exit_instruction_pointer: None,
                source: InterruptSource::External,
                interrupted_continuation_id: Uuid::nil(),
                resume_instruction_pointer: 7,
                payload: Value::from("payload"),
                handled_at_instruction_pointers: vec![7],
            }),
            WorkflowFrame::Compensation(WorkflowCompensationFrame {
                pending: vec![WorkflowEffectRequest::Timer { due_at: 1 }],
                active: None,
                resume: Some(8),
            }),
            WorkflowFrame::Invocation(WorkflowInvocationFrame {
                module: InvocationModule::new(Default::default()),
                continuation: crate::invocation::InvocationContinuation::start(),
            }),
            WorkflowFrame::Debug(WorkflowDebugFrame {
                paused: true,
                step_requested: true,
                breakpoint: Some("node:publish".into()),
                last_output: Some(Value::from("output")),
                speculative: true,
            }),
        ];

        let encoded = serde_json::to_string(&continuation).unwrap();
        let decoded: WorkflowContinuation = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, continuation);
        assert!(!encoded.contains("RunCursor"));
        assert!(!encoded.contains("node_run"));
    }
}
