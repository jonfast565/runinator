//! Durable host contract for the compiled workflow VM.
//!
//! The important operation is [`WorkflowVmStore::suspend_on_effect`]: it updates the frozen
//! continuation, records its uniquely-identified effect and queues its external delivery in one
//! transaction. Implementations must make a duplicate `(continuation_id, sequence)` a no-op.

use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_comm::{EffectCommand, EffectDispatchRecord};
use runinator_models::{
    errors::SendableError,
    replicas::WorkflowRunProvenance,
    value::Value,
    workflow_vm::{
        WorkflowContinuation, WorkflowEffect, WorkflowEffectOutputEvent, WorkflowEffectStatus,
        WorkflowInterruptOutcome, WorkflowJournalEntry, WorkflowJournalRecord, WorkflowModule,
        WorkflowPendingInterrupt,
    },
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus},
};
use uuid::Uuid;

/// One durable periodic timer attached to a running workflow. The schedule is separate from a
/// continuation so a timer survives forks, handler completion, and the continuation it happens to
/// interrupt changing over time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTimerInterrupt {
    pub workflow_run_id: Uuid,
    pub timer_id: String,
    pub interval_seconds: i64,
    pub due_at: DateTime<Utc>,
}

/// Everything needed to freeze a new VM-backed workflow run in one transaction.
#[derive(Debug, Clone)]
pub struct NewWorkflowVmRun {
    pub workflow_id: Uuid,
    pub workflow_snapshot: WorkflowDefinition,
    pub parameters: Value,
    /// Eager configuration snapshot exposed to bytecode as the `config` local. Resolving it when
    /// the run starts makes retries and resumed continuations independent of later setting edits.
    pub config: Value,
    pub state: Value,
    pub name: Option<String>,
    pub provenance: WorkflowRunProvenance,
    /// Owning pipeline run when this is a pipeline member. This is persisted with the run row
    /// inside the same transaction as the module, root continuation, and first journal entry.
    pub pipeline_run_id: Option<Uuid>,
    /// Pipeline member attempt to bind to the new run in that same transaction.
    pub pipeline_member_attempt_id: Option<Uuid>,
    pub module: WorkflowModule,
    /// Initial bytecode location. Zero starts normally; replay uses a source-map boundary.
    pub instruction_pointer: usize,
}

/// Persistence used only by the compiled workflow runtime and its durable host.
pub trait WorkflowVmStore: Send + Sync + 'static {
    /// Atomically create the public run row, frozen module, root continuation, and first journal
    /// entry. No caller may publish or return a new VM run before all four records exist.
    fn create_workflow_vm_run(
        &self,
        start: NewWorkflowVmRun,
    ) -> impl Future<Output = Result<WorkflowRun, SendableError>> + Send;

    fn fetch_workflow_module(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowModule>, SendableError>> + Send;

    fn fetch_workflow_continuation(
        &self,
        continuation_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowContinuation>, SendableError>> + Send;

    /// All durable branches of one workflow run, in their creation order.
    fn fetch_workflow_continuations(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowContinuation>, SendableError>> + Send;

    fn fetch_workflow_effect(
        &self,
        effect_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowEffect>, SendableError>> + Send;

    /// Durable effect receipts for one workflow run, ordered by creation and identity.
    fn fetch_workflow_effects(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowEffect>, SendableError>> + Send;

    /// Ordered streamed output for one effect. `event_id` is the broker-delivery dedupe key.
    fn fetch_workflow_effect_output(
        &self,
        effect_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowEffectOutputEvent>, SendableError>> + Send;

    /// Persist one chunk/artifact exactly once. Returns false for a duplicate event or stale
    /// effect attempt.
    fn append_workflow_effect_output(
        &self,
        event: WorkflowEffectOutputEvent,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Persist a terminal input lifecycle event and project the newest attempt-local sequence onto
    /// the effect/run status. Older or duplicate broker deliveries remain history but cannot move
    /// the live effect back to a stale state.
    fn record_workflow_terminal_interaction(
        &self,
        event: WorkflowEffectOutputEvent,
        recorded_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn fetch_workflow_journal(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowJournalRecord>, SendableError>> + Send;

    /// Immutable VM journal records for every run of one workflow definition. Records are grouped
    /// by run and ordered by their durable per-run sequence so callers can reconstruct each
    /// continuation's graph path without consulting legacy node-run rows.
    fn fetch_workflow_journals_for_workflow(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowJournalRecord>, SendableError>> + Send;

    /// Settle the public run after all of its continuations are terminal.
    fn settle_workflow_vm_run(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Find terminal VM pipeline members whose pipeline attempt has not observed settlement yet.
    /// This is the crash-recovery seam between workflow settlement and pipeline advancement.
    fn fetch_unsettled_vm_pipeline_members(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Uuid>, SendableError>> + Send;

    /// Persist a new root continuation and its initial journal entry.
    fn create_workflow_vm(
        &self,
        module: WorkflowModule,
        continuation: WorkflowContinuation,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Atomically save a waiting continuation, insert an effect receipt, append journal records,
    /// and enqueue `command` for delivery. A duplicate effect sequence returns the existing
    /// receipt and must not enqueue a second command.
    fn suspend_on_effect(
        &self,
        continuation: WorkflowContinuation,
        effect: WorkflowEffect,
        command: EffectCommand,
    ) -> impl Future<Output = Result<WorkflowEffect, SendableError>> + Send;

    /// Atomically persist a fork: parent state, all child continuations, and its journal record.
    fn fork_workflow_continuation(
        &self,
        parent: WorkflowContinuation,
        children: Vec<WorkflowContinuation>,
        join_key: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Compare-and-swap a continuation transition and append its history boundary in the same
    /// transaction. This is the common write for join arrival, race resolution, cancellation,
    /// and terminal VM outcomes; callers supply the transition computed from the frozen module.
    fn commit_workflow_continuation(
        &self,
        continuation: WorkflowContinuation,
        journal: WorkflowJournalEntry,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Atomically freeze one thread and start its handler continuation beside it. Both records
    /// move in a single transaction: a suspended thread with no handler would never run again.
    fn raise_workflow_interrupt(
        &self,
        suspended: WorkflowContinuation,
        handler: WorkflowContinuation,
        journal: WorkflowJournalEntry,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Start a handler continuation beside a thread that is *not* being suspended, for a source
    /// raised while the thread is already parked on an effect. Inserting the handler is idempotent
    /// on its id, so a redelivered result cannot start the same handler twice.
    fn start_workflow_interrupt_handler(
        &self,
        handler: WorkflowContinuation,
        journal: WorkflowJournalEntry,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Retire a finished handler and apply its decision to the thread it suspended.
    fn settle_workflow_interrupt(
        &self,
        handler: WorkflowContinuation,
        interrupted_continuation_id: Uuid,
        outcome: WorkflowInterruptOutcome,
        journal: WorkflowJournalEntry,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Record an out-of-band interrupt request on one thread of a run, to be raised or refused by
    /// the next drive of that thread. `continuation_id` selects a thread; `None` targets the run's
    /// oldest live one. Returns the continuation it landed on.
    fn request_workflow_interrupt(
        &self,
        workflow_run_id: Uuid,
        continuation_id: Option<Uuid>,
        pending: WorkflowPendingInterrupt,
    ) -> impl Future<Output = Result<Option<Uuid>, SendableError>> + Send;

    /// List timer occurrences that should be armed with the broker-only waker. Repeated arming of
    /// the same row is safe: its wake dedupe key includes the exact `due_at` instant.
    fn fetch_workflow_timer_interrupts_before(
        &self,
        before: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowTimerInterrupt>, SendableError>> + Send;

    /// Apply a due timer once: advance the durable schedule and, when a real cursor is available,
    /// record the matching pending interrupt on it in the same transaction. Returns false for a
    /// stale/redelivered wake or a run that has already finished.
    fn fire_workflow_timer_interrupt(
        &self,
        timer: WorkflowTimerInterrupt,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Record the replica executing one effect attempt, and mark the effect running. Returns
    /// false for a stale attempt or an already-terminal effect. This is the VM's executor lease:
    /// replica load and stale-replica reaping read it instead of the removed node-run columns.
    fn claim_workflow_effect_executor(
        &self,
        effect_id: Uuid,
        attempt: u32,
        replica_id: Uuid,
        claimed_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Re-arm a failed effect for another attempt instead of settling it: bump the attempt, clear
    /// the previous outcome and executor lease, and queue a fresh delayed dispatch of the *frozen*
    /// command that is not claimable before `available_at`. Returns false for a stale attempt or an
    /// already-terminal effect, so a duplicate result cannot schedule a second retry.
    fn retry_workflow_effect(
        &self,
        effect_id: Uuid,
        attempt: u32,
        available_at: DateTime<Utc>,
        message: Option<String>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Settle one effect exactly once. Returns false for stale attempts and already-terminal
    /// effects; a successful settlement must enqueue the addressed continuation for a drive.
    fn settle_workflow_effect(
        &self,
        effect_id: Uuid,
        attempt: u32,
        status: WorkflowEffectStatus,
        output: Option<Value>,
        message: Option<String>,
        settled_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Commit portable contents in the same transaction that releases the continuation.
    fn settle_workflow_effect_with_workspace(
        &self,
        settlement: WorkspaceEffectSettlement,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send {
        async move {
            if settlement.workspace.is_some() {
                return Err(runinator_models::errors::WORKSPACE_COMMIT_UNSUPPORTED
                    .error("store has no workspace transaction"));
            }
            self.settle_workflow_effect(
                settlement.effect_id,
                settlement.attempt,
                settlement.status,
                settlement.output,
                settlement.message,
                settlement.settled_at,
            )
            .await
        }
    }

    /// Pause every runnable continuation in a run, without touching continuations waiting on an
    /// external effect. Returns the number of continuations changed.
    fn pause_workflow_vm_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Resume operator-paused continuations. When `step` is true each resumed continuation parks
    /// at its next debugger boundary.
    fn resume_workflow_vm_run(
        &self,
        workflow_run_id: Uuid,
        step: bool,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Atomically cancel all nonterminal continuations and unsettled effects and settle the public
    /// workflow run. Returns effect ids whose provider executions may need cooperative cancel.
    fn cancel_workflow_vm_run(
        &self,
        workflow_run_id: Uuid,
        message: String,
    ) -> impl Future<Output = Result<Vec<Uuid>, SendableError>> + Send;

    /// Atomically acquire a named run-scoped mutex without creating a legacy node-run waiter.
    fn claim_workflow_vm_mutex(
        &self,
        name: String,
        workflow_run_id: Uuid,
        continuation_id: Uuid,
        now: i64,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Release a named mutex when this continuation owns it. A release that was already applied
    /// is a no-op, which keeps a redelivered infrastructure effect safe.
    fn release_workflow_vm_mutex(
        &self,
        name: String,
        workflow_run_id: Uuid,
        continuation_id: Uuid,
        now: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Lease runnable continuations for a machine drive.
    fn claim_runnable_workflow_continuations(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowContinuation>, SendableError>> + Send;

    /// Lease unpublished effect commands for one broker publisher. A lease expiry makes an
    /// unacknowledged command available again, preserving at-least-once delivery after a crash.
    fn claim_pending_workflow_effect_dispatches(
        &self,
        publisher_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<EffectDispatchRecord>, SendableError>> + Send;

    /// Acknowledge publication of a claimed VM effect command.
    fn mark_workflow_effect_dispatch_published(
        &self,
        dispatch_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Release a VM effect command after a failed broker publication so it can be retried.
    fn mark_workflow_effect_dispatch_failed(
        &self,
        dispatch_id: Uuid,
        error: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}

/// An effect completion and its optional portable workspace snapshot.
pub struct WorkspaceEffectSettlement {
    pub effect_id: Uuid,
    pub attempt: u32,
    pub status: WorkflowEffectStatus,
    pub output: Option<Value>,
    pub message: Option<String>,
    pub settled_at: DateTime<Utc>,
    pub workspace: Option<runinator_models::workspaces::WorkspaceCommit>,
}
