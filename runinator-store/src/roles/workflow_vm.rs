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
    value::Value,
    workflow_vm::{
        WorkflowContinuation, WorkflowEffect, WorkflowEffectStatus, WorkflowJournalEntry,
        WorkflowJournalRecord, WorkflowModule,
    },
};
use uuid::Uuid;

/// Persistence used only by the compiled workflow runtime and its durable host.
pub trait WorkflowVmStore: Send + Sync + 'static {
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

    fn fetch_workflow_journal(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowJournalRecord>, SendableError>> + Send;

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
