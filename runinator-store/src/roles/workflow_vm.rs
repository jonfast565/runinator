//! Durable host contract for the compiled workflow VM.
//!
//! The important operation is [`WorkflowVmStore::suspend_on_effect`]: it updates the frozen
//! continuation, records its uniquely-identified effect and queues its external delivery in one
//! transaction. Implementations must make a duplicate `(continuation_id, sequence)` a no-op.

use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_comm::EffectCommand;
use runinator_models::{
    errors::SendableError,
    value::Value,
    workflow_vm::{
        WorkflowContinuation, WorkflowEffect, WorkflowEffectStatus, WorkflowJournalRecord,
        WorkflowModule,
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

    fn fetch_workflow_effect(
        &self,
        effect_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowEffect>, SendableError>> + Send;

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
}
