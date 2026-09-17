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
use crate::workspaces::WORKSPACE_TIMELINE_STREAM;
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

/// Backend-owned classification used by operator timelines and other event consumers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTimelineCategory {
    #[default]
    User,
    System,
}

/// Local-slot prefix for compiled node outputs exposed through `steps.<node>.output`.
pub const WORKFLOW_NODE_OUTPUT_PREFIX: &str = "__workflow_vm_node_output:";

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
    Compensation(Box<WorkflowCompensationFrame>),
    Invocation(WorkflowInvocationFrame),
    Debug(WorkflowDebugFrame),
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
    /// Structured provider progress retained beside ordinary output chunks. The event kind is
    /// provider-owned; consumers must treat its payload as untrusted display data.
    Progress {
        kind: String,
        payload: Value,
    },
    TerminalInteraction {
        interaction: crate::runs::TerminalInteraction,
    },
}

impl WorkflowEffectOutput {
    pub fn timeline_category(&self) -> WorkflowTimelineCategory {
        match self {
            Self::Chunk { stream, .. } if stream == WORKSPACE_TIMELINE_STREAM => {
                WorkflowTimelineCategory::System
            }
            Self::Chunk { .. }
            | Self::Artifact { .. }
            | Self::Progress { .. }
            | Self::TerminalInteraction { .. } => WorkflowTimelineCategory::User,
        }
    }
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
        /// Stable execution-profile binding selected by the authored action.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        execution_profile: Option<crate::execution_profiles::ExecutionProfileBinding>,
        /// A still-unresolved key expression. The VM host evaluates and records it before the
        /// effect is delivered, so redelivery cannot observe a changed workflow definition.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idempotency_key: Option<Value>,
        /// Packaged functions are addressed by their immutable binding, not by a provider catalog
        /// lookup that may have changed between compile and delivery.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        function_binding: Option<Box<FunctionBinding>>,
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

impl WorkflowEffectRequest {
    pub fn timeline_category(&self) -> WorkflowTimelineCategory {
        match self {
            Self::Action { .. }
            | Self::Timer { .. }
            | Self::TimerDelay { .. }
            | Self::Approval { .. }
            | Self::Gate { .. }
            | Self::Signal { .. }
            | Self::Input { .. }
            | Self::EventWait { .. }
            | Self::ChildRun { .. }
            | Self::AwaitRun { .. }
            | Self::MutexAcquire { .. }
            | Self::Coordination { .. } => WorkflowTimelineCategory::User,
        }
    }
}

impl WorkflowJournalEntry {
    pub fn timeline_category(&self) -> WorkflowTimelineCategory {
        match self {
            Self::NodeEntered { .. } | Self::EffectRetryScheduled { .. } | Self::Failed { .. } => {
                WorkflowTimelineCategory::User
            }
            Self::Entered { .. }
            | Self::Transitioned { .. }
            | Self::Forked { .. }
            | Self::EffectRequested { .. }
            | Self::EffectSettled { .. }
            | Self::Completed { .. }
            | Self::Interrupted { .. }
            | Self::InterruptResolved { .. } => WorkflowTimelineCategory::System,
        }
    }
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
            timeline_category: WorkflowTimelineCategory::User,
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
            timeline_category: WorkflowTimelineCategory::User,
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
            timeline_category: WorkflowTimelineCategory::System,
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
            r#"{"version":1,"id":"00000000-0000-0000-0000-000000000000","workflow_run_id":"00000000-0000-0000-0000-000000000000","continuation_id":"00000000-0000-0000-0000-000000000000","sequence":0,"attempt":0,"timeline_category":"user","request":{"type":"timer","due_at":1},"status":"requested","created_at":0,"updated_at":0}"#
        );
        assert_eq!(
            serde_json::to_string(&journal).unwrap(),
            r#"{"version":1,"id":"00000000-0000-0000-0000-000000000000","workflow_run_id":"00000000-0000-0000-0000-000000000000","sequence":0,"continuation_id":"00000000-0000-0000-0000-000000000000","timeline_category":"system","entry":{"type":"entered","continuation_id":"00000000-0000-0000-0000-000000000000","instruction_pointer":0},"created_at":0}"#
        );
    }

    #[test]
    fn timeline_categories_are_owned_by_backend_event_semantics() {
        let workspace = WorkflowEffectOutput::Chunk {
            stream: WORKSPACE_TIMELINE_STREAM.into(),
            content: "{}".into(),
        };
        let ordinary_output = WorkflowEffectOutput::Chunk {
            stream: "stdout".into(),
            content: "hello".into(),
        };

        assert_eq!(
            workspace.timeline_category(),
            WorkflowTimelineCategory::System
        );
        assert_eq!(
            ordinary_output.timeline_category(),
            WorkflowTimelineCategory::User
        );
        assert_eq!(
            WorkflowEffectRequest::Timer { due_at: 1 }.timeline_category(),
            WorkflowTimelineCategory::User
        );
        let user_entries = [
            WorkflowJournalEntry::NodeEntered {
                continuation_id: Uuid::nil(),
                node_id: "node".into(),
            },
            WorkflowJournalEntry::EffectRetryScheduled {
                effect_id: Uuid::nil(),
                attempt: 2,
                available_at: 1,
            },
            WorkflowJournalEntry::Failed {
                continuation_id: Uuid::nil(),
                message: "failed".into(),
                node_id: Some("node".into()),
            },
        ];
        assert!(
            user_entries
                .iter()
                .all(|entry| entry.timeline_category() == WorkflowTimelineCategory::User)
        );

        let system_entries = [
            WorkflowJournalEntry::Entered {
                continuation_id: Uuid::nil(),
                instruction_pointer: 0,
            },
            WorkflowJournalEntry::Transitioned {
                continuation_id: Uuid::nil(),
                instruction_pointer: 1,
            },
            WorkflowJournalEntry::Forked {
                continuation_id: Uuid::nil(),
                children: vec![Uuid::nil()],
                join_key: "join".into(),
            },
            WorkflowJournalEntry::EffectRequested {
                effect_id: Uuid::nil(),
                instruction_pointer: Some(1),
            },
            WorkflowJournalEntry::EffectSettled {
                effect_id: Uuid::nil(),
                status: WorkflowEffectStatus::Succeeded,
            },
            WorkflowJournalEntry::Completed {
                continuation_id: Uuid::nil(),
                value: Value::Null,
            },
            WorkflowJournalEntry::Interrupted {
                continuation_id: Uuid::nil(),
                handler_continuation_id: Uuid::nil(),
                source: InterruptSource::Timer,
            },
            WorkflowJournalEntry::InterruptResolved {
                continuation_id: Uuid::nil(),
                handler_continuation_id: Uuid::nil(),
                outcome: WorkflowInterruptOutcome::Resume {
                    instruction_pointer: 1,
                },
            },
        ];
        assert!(
            system_entries
                .iter()
                .all(|entry| entry.timeline_category() == WorkflowTimelineCategory::System)
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
            WorkflowFrame::Compensation(Box::new(WorkflowCompensationFrame {
                pending: vec![WorkflowEffectRequest::Timer { due_at: 1 }],
                active: None,
                resume: Some(8),
            })),
            WorkflowFrame::Invocation(WorkflowInvocationFrame {
                module: InvocationModule::new(Default::default()),
                continuation: crate::invocation::InvocationContinuation::start(),
            }),
            WorkflowFrame::Debug(WorkflowDebugFrame {
                paused: true,
                step_requested: true,
                breakpoint: Some("node:publish".into()),
                run_to_node_id: Some("node:finish".into()),
                pending_failure: None,
                pause_on_failure: true,
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

mod unsupported_workflow_vm_version;
pub use unsupported_workflow_vm_version::UnsupportedWorkflowVmVersion;

mod workflow_module;
pub use workflow_module::WorkflowModule;

mod workflow_source_map_entry;
pub use workflow_source_map_entry::WorkflowSourceMapEntry;

mod workflow_vm_branch;
pub use workflow_vm_branch::WorkflowVmBranch;

mod workflow_output_artifact;
pub use workflow_output_artifact::WorkflowOutputArtifact;

mod workflow_pending_interrupt;
pub use workflow_pending_interrupt::WorkflowPendingInterrupt;

mod workflow_vm_interrupt_handler;
pub use workflow_vm_interrupt_handler::WorkflowVmInterruptHandler;

mod workflow_loop_frame;
pub use workflow_loop_frame::WorkflowLoopFrame;

mod workflow_reentry_frame;
pub use workflow_reentry_frame::WorkflowReentryFrame;

mod workflow_failure;
pub use workflow_failure::WorkflowFailure;

mod workflow_try_frame;
pub use workflow_try_frame::WorkflowTryFrame;

mod workflow_map_frame;
pub use workflow_map_frame::WorkflowMapFrame;

mod workflow_indexed_value;
pub use workflow_indexed_value::WorkflowIndexedValue;

mod workflow_fork_frame;
pub use workflow_fork_frame::WorkflowForkFrame;

mod workflow_join_frame;
pub use workflow_join_frame::WorkflowJoinFrame;

mod workflow_race_frame;
pub use workflow_race_frame::WorkflowRaceFrame;

mod workflow_interrupt_frame;
pub use workflow_interrupt_frame::WorkflowInterruptFrame;

mod workflow_compensation_frame;
pub use workflow_compensation_frame::WorkflowCompensationFrame;

mod workflow_invocation_frame;
pub use workflow_invocation_frame::WorkflowInvocationFrame;

mod workflow_debug_frame;
pub use workflow_debug_frame::WorkflowDebugFrame;

mod workflow_continuation;
pub use workflow_continuation::WorkflowContinuation;

mod workflow_vm_cursor;
pub use workflow_vm_cursor::WorkflowVmCursor;

mod workflow_effect_output_event;
pub use workflow_effect_output_event::WorkflowEffectOutputEvent;

mod workflow_effect;
pub use workflow_effect::WorkflowEffect;

mod workflow_journal_record;
pub use workflow_journal_record::WorkflowJournalRecord;
