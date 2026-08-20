//! Durable host for the compiled workflow VM.
//!
//! The interpreter stays pure. This module turns its boundaries into store transactions; effect
//! delivery itself remains an outbox concern.

use chrono::{Duration, Utc};
use runinator_comm::{ActionTarget, EffectCommand, EffectExecutor};
use runinator_models::{
    errors::SendableError,
    value::Value,
    workflow_vm::{
        WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowContinuation, WorkflowContinuationStatus,
        WorkflowEffect, WorkflowEffectStatus, WorkflowJournalEntry,
    },
    workflows::WorkflowStatus,
};
use runinator_store::roles::WorkflowVmStore;
use uuid::Uuid;

use crate::{
    WorkflowVmStep,
    errors::{WORKFLOW_VM_EFFECT_MISSING, WORKFLOW_VM_MODULE_MISSING},
    resume_workflow_vm, step_workflow_vm,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowVmDriveOutcome {
    Yielded,
    Forked,
    Joined,
    Completed { settled_run_id: Option<Uuid> },
    Failed { settled_run_id: Option<Uuid> },
}

/// Drives continuations leased by a scheduler through their snapshotted workflow modules.
pub struct WorkflowVmHost<'a, S: WorkflowVmStore> {
    store: &'a S,
}

impl<'a, S: WorkflowVmStore> WorkflowVmHost<'a, S> {
    pub fn new(store: &'a S) -> Self {
        Self { store }
    }

    pub async fn drive_runnable(
        &self,
        scheduler_id: String,
        limit: i64,
    ) -> Result<Vec<WorkflowVmDriveOutcome>, SendableError> {
        let now = Utc::now();
        let claimed = self
            .store
            .claim_runnable_workflow_continuations(
                scheduler_id,
                now,
                now + Duration::seconds(30),
                limit,
            )
            .await?;
        let mut outcomes = Vec::with_capacity(claimed.len());
        for continuation in claimed {
            outcomes.push(self.drive_claimed(continuation).await?);
        }
        Ok(outcomes)
    }

    /// Drive one continuation already claimed by this scheduler.
    pub async fn drive_claimed(
        &self,
        continuation: WorkflowContinuation,
    ) -> Result<WorkflowVmDriveOutcome, SendableError> {
        let module = self
            .store
            .fetch_workflow_module(continuation.workflow_run_id)
            .await?
            .ok_or_else(|| WORKFLOW_VM_MODULE_MISSING.error(continuation.workflow_run_id))?;
        let step = if let Some(effect_id) = continuation.awaiting_effect_id {
            let effect = self
                .store
                .fetch_workflow_effect(effect_id)
                .await?
                .ok_or_else(|| WORKFLOW_VM_EFFECT_MISSING.error(effect_id))?;
            let result = match effect.status {
                WorkflowEffectStatus::Succeeded => Ok(effect.result.unwrap_or(Value::Null)),
                WorkflowEffectStatus::Failed => {
                    Err(effect.message.unwrap_or_else(|| "effect failed".into()))
                }
                WorkflowEffectStatus::TimedOut => {
                    Err(effect.message.unwrap_or_else(|| "effect timed out".into()))
                }
                WorkflowEffectStatus::Canceled => {
                    Err(effect.message.unwrap_or_else(|| "effect canceled".into()))
                }
                WorkflowEffectStatus::Requested | WorkflowEffectStatus::Running => {
                    return Err(WORKFLOW_VM_EFFECT_MISSING
                        .error(format!("effect {effect_id} was claimed before settlement")));
                }
            };
            resume_workflow_vm(&module, continuation, result)
        } else {
            step_workflow_vm(&module, continuation)
        };
        self.apply(step).await
    }

    async fn apply(&self, step: WorkflowVmStep) -> Result<WorkflowVmDriveOutcome, SendableError> {
        match step {
            WorkflowVmStep::Yield {
                continuation,
                effect_id,
                sequence,
                request,
            } => {
                let now = Utc::now().timestamp();
                let target = match &request {
                    runinator_models::workflow_vm::WorkflowEffectRequest::Action {
                        required_labels,
                        ..
                    } if !required_labels.is_empty() => {
                        ActionTarget::labels(required_labels.clone())
                    }
                    _ => ActionTarget::Any,
                };
                let executor = if matches!(
                    &request,
                    runinator_models::workflow_vm::WorkflowEffectRequest::Action { .. }
                ) {
                    EffectExecutor::Provider
                } else {
                    EffectExecutor::Infrastructure
                };
                let effect = WorkflowEffect {
                    version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
                    id: effect_id,
                    workflow_run_id: continuation.workflow_run_id,
                    continuation_id: continuation.id,
                    sequence,
                    attempt: 0,
                    request: request.clone(),
                    status: WorkflowEffectStatus::Requested,
                    result: None,
                    message: None,
                    created_at: now,
                    updated_at: now,
                    finished_at: None,
                };
                let command = EffectCommand {
                    version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
                    command_id: Uuid::now_v7(),
                    effect_id,
                    workflow_run_id: continuation.workflow_run_id,
                    continuation_id: continuation.id,
                    attempt: 0,
                    request,
                    executor,
                    target,
                    trace_id: Uuid::now_v7(),
                    trace_context: Default::default(),
                    idempotency_key: effect.idempotency_key(),
                };
                self.store
                    .suspend_on_effect(continuation, effect, command)
                    .await?;
                Ok(WorkflowVmDriveOutcome::Yielded)
            }
            WorkflowVmStep::Fork {
                parent,
                children,
                join_key,
            } => {
                self.store
                    .fork_workflow_continuation(parent, children, join_key)
                    .await?;
                Ok(WorkflowVmDriveOutcome::Forked)
            }
            WorkflowVmStep::Joined { continuation, .. } => {
                let journal = WorkflowJournalEntry::Transitioned {
                    continuation_id: continuation.id,
                    instruction_pointer: continuation.instruction_pointer,
                };
                self.store
                    .commit_workflow_continuation(continuation, journal)
                    .await?;
                Ok(WorkflowVmDriveOutcome::Joined)
            }
            WorkflowVmStep::Complete {
                continuation,
                value,
            } => {
                let run_id = continuation.workflow_run_id;
                self.store
                    .commit_workflow_continuation(
                        continuation.clone(),
                        WorkflowJournalEntry::Completed {
                            continuation_id: continuation.id,
                            value,
                        },
                    )
                    .await?;
                let settled_run_id = self.settle_run_if_terminal(run_id).await?.then_some(run_id);
                Ok(WorkflowVmDriveOutcome::Completed { settled_run_id })
            }
            WorkflowVmStep::Failed {
                continuation,
                message,
            } => {
                let run_id = continuation.workflow_run_id;
                self.store
                    .commit_workflow_continuation(
                        continuation.clone(),
                        WorkflowJournalEntry::Failed {
                            continuation_id: continuation.id,
                            message,
                        },
                    )
                    .await?;
                let settled_run_id = self.settle_run_if_terminal(run_id).await?.then_some(run_id);
                Ok(WorkflowVmDriveOutcome::Failed { settled_run_id })
            }
        }
    }

    async fn settle_run_if_terminal(&self, workflow_run_id: Uuid) -> Result<bool, SendableError> {
        let continuations = self
            .store
            .fetch_workflow_continuations(workflow_run_id)
            .await?;
        if continuations.is_empty() || continuations.iter().any(|c| !c.status.is_terminal()) {
            return Ok(false);
        }
        let (status, message) = if continuations
            .iter()
            .any(|c| c.status == WorkflowContinuationStatus::Failed)
        {
            (WorkflowStatus::Failed, Some("VM workflow failed".into()))
        } else if continuations
            .iter()
            .any(|c| c.status == WorkflowContinuationStatus::Canceled)
        {
            (
                WorkflowStatus::Canceled,
                Some("VM workflow canceled".into()),
            )
        } else {
            (WorkflowStatus::Succeeded, None)
        };
        self.store
            .settle_workflow_vm_run(workflow_run_id, status, message)
            .await?;
        Ok(true)
    }
}
