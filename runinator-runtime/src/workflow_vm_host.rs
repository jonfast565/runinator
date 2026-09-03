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
        WorkflowEffect, WorkflowEffectRequest, WorkflowEffectStatus, WorkflowFailure,
        WorkflowFailureKind, WorkflowJournalEntry,
    },
    workflows::WorkflowStatus,
    workspaces::{WORKSPACE_INSTANCE_LABEL, WorkspaceAffinity},
};
use runinator_store::{RuntimeStore, roles::WorkflowVmStore};
use uuid::Uuid;

use crate::{
    WorkflowVmStep,
    errors::{WORKFLOW_VM_EFFECT_MISSING, WORKFLOW_VM_MODULE_MISSING},
    resume_workflow_vm_with_debug, step_workflow_vm_with_debug,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowVmDriveOutcome {
    Yielded {
        workflow_run_id: Uuid,
    },
    Forked {
        workflow_run_id: Uuid,
    },
    Joined {
        workflow_run_id: Uuid,
    },
    Completed {
        workflow_run_id: Uuid,
        settled_run_id: Option<Uuid>,
    },
    Failed {
        workflow_run_id: Uuid,
        settled_run_id: Option<Uuid>,
    },
    Interrupted {
        workflow_run_id: Uuid,
    },
    InterruptResolved {
        workflow_run_id: Uuid,
        settled_run_id: Option<Uuid>,
    },
}

impl WorkflowVmDriveOutcome {
    pub fn workflow_run_id(&self) -> Uuid {
        match self {
            Self::Yielded { workflow_run_id }
            | Self::Forked { workflow_run_id }
            | Self::Joined { workflow_run_id }
            | Self::Completed {
                workflow_run_id, ..
            }
            | Self::Failed {
                workflow_run_id, ..
            }
            | Self::Interrupted { workflow_run_id }
            | Self::InterruptResolved {
                workflow_run_id, ..
            } => *workflow_run_id,
        }
    }
}

/// Drives continuations leased by a scheduler through their snapshotted workflow modules.
pub struct WorkflowVmHost<'a, S: WorkflowVmStore + RuntimeStore> {
    store: &'a S,
}

impl<'a, S: WorkflowVmStore + RuntimeStore> WorkflowVmHost<'a, S> {
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
        let debug = self
            .store
            .fetch_workflow_run(continuation.workflow_run_id)
            .await?
            .and_then(|run| run.execution_state.debug)
            .map(|frame| frame.config);
        let step = if let Some(effect_id) = continuation.awaiting_effect_id {
            let effect = self
                .store
                .fetch_workflow_effect(effect_id)
                .await?
                .ok_or_else(|| WORKFLOW_VM_EFFECT_MISSING.error(effect_id))?;
            // the classification, not only the message: the graph routes `on_timeout` and
            // `on_reject` apart from `on_failure`, and only the effect's terminal status says
            // which of the three this is.
            let result = match effect.status {
                WorkflowEffectStatus::Succeeded => Ok(effect.result.unwrap_or(Value::Null)),
                WorkflowEffectStatus::Failed => Err(WorkflowFailure::new(
                    WorkflowFailureKind::Failed,
                    effect.message.unwrap_or_else(|| "effect failed".into()),
                )),
                WorkflowEffectStatus::Rejected => Err(WorkflowFailure::new(
                    WorkflowFailureKind::Rejected,
                    effect.message.unwrap_or_else(|| "effect rejected".into()),
                )),
                WorkflowEffectStatus::TimedOut => Err(WorkflowFailure::new(
                    WorkflowFailureKind::TimedOut,
                    effect.message.unwrap_or_else(|| "effect timed out".into()),
                )),
                WorkflowEffectStatus::Canceled => Err(WorkflowFailure::new(
                    WorkflowFailureKind::Canceled,
                    effect.message.unwrap_or_else(|| "effect canceled".into()),
                )),
                WorkflowEffectStatus::Requested
                | WorkflowEffectStatus::Running
                | WorkflowEffectStatus::InputRequired => {
                    return Err(WORKFLOW_VM_EFFECT_MISSING
                        .error(format!("effect {effect_id} was claimed before settlement")));
                }
            };
            resume_workflow_vm_with_debug(
                &module,
                continuation,
                Some(&effect.request),
                result,
                debug.as_ref(),
            )
        } else {
            step_workflow_vm_with_debug(&module, continuation, debug.as_ref())
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
                let workflow_run_id = continuation.workflow_run_id;
                let now = Utc::now().timestamp();
                let target = effect_target(&request)?;
                let executor = if matches!(
                    request.as_ref(),
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
                    node_id: None,
                    request: request.as_ref().clone(),
                    status: WorkflowEffectStatus::Requested,
                    current_executor_replica_id: None,
                    last_executor_replica_id: None,
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
                    request: *request,
                    executor,
                    target,
                    trace_id: Uuid::now_v7(),
                    trace_context: Default::default(),
                    idempotency_key: effect.idempotency_key(),
                    notification_delivery_id: None,
                };
                self.store
                    .suspend_on_effect(continuation, effect, command)
                    .await?;
                Ok(WorkflowVmDriveOutcome::Yielded { workflow_run_id })
            }
            WorkflowVmStep::Fork {
                parent,
                children,
                join_key,
            } => {
                let workflow_run_id = parent.workflow_run_id;
                self.store
                    .fork_workflow_continuation(parent, children, join_key)
                    .await?;
                Ok(WorkflowVmDriveOutcome::Forked { workflow_run_id })
            }
            WorkflowVmStep::Joined { continuation, .. } => {
                let workflow_run_id = continuation.workflow_run_id;
                let journal = WorkflowJournalEntry::Transitioned {
                    continuation_id: continuation.id,
                    instruction_pointer: continuation.instruction_pointer,
                };
                self.store
                    .commit_workflow_continuation(continuation, journal)
                    .await?;
                Ok(WorkflowVmDriveOutcome::Joined { workflow_run_id })
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
                Ok(WorkflowVmDriveOutcome::Completed {
                    workflow_run_id: run_id,
                    settled_run_id,
                })
            }
            WorkflowVmStep::Failed {
                continuation,
                message,
            } => {
                let run_id = continuation.workflow_run_id;
                let node_id = continuation.pending_node_entries.last().cloned();
                self.store
                    .commit_workflow_continuation(
                        continuation.clone(),
                        WorkflowJournalEntry::Failed {
                            continuation_id: continuation.id,
                            message,
                            node_id,
                        },
                    )
                    .await?;
                let settled_run_id = self.settle_run_if_terminal(run_id).await?.then_some(run_id);
                Ok(WorkflowVmDriveOutcome::Failed {
                    workflow_run_id: run_id,
                    settled_run_id,
                })
            }
            WorkflowVmStep::Interrupted {
                suspended,
                handler,
                source,
            } => {
                let workflow_run_id = suspended.workflow_run_id;
                let journal = WorkflowJournalEntry::Interrupted {
                    continuation_id: suspended.id,
                    handler_continuation_id: handler.id,
                    source,
                };
                self.store
                    .raise_workflow_interrupt(suspended, *handler, journal)
                    .await?;
                Ok(WorkflowVmDriveOutcome::Interrupted { workflow_run_id })
            }
            WorkflowVmStep::InterruptResolved {
                handler,
                interrupted_continuation_id,
                outcome,
            } => {
                let run_id = handler.workflow_run_id;
                let journal = WorkflowJournalEntry::InterruptResolved {
                    continuation_id: interrupted_continuation_id,
                    handler_continuation_id: handler.id,
                    outcome: outcome.clone(),
                };
                self.store
                    .settle_workflow_interrupt(
                        handler,
                        interrupted_continuation_id,
                        outcome,
                        journal,
                    )
                    .await?;
                let settled_run_id = self.settle_run_if_terminal(run_id).await?.then_some(run_id);
                Ok(WorkflowVmDriveOutcome::InterruptResolved {
                    workflow_run_id: run_id,
                    settled_run_id,
                })
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
        // a handler is excluded from the vote: a region that broke on its own must not turn into a
        // failed run, and one that finished cleanly must not turn a failing run into a passing one.
        let deciding = continuations
            .iter()
            .filter(|c| !c.is_interrupt_handler())
            .collect::<Vec<_>>();
        let (status, message) = if deciding
            .iter()
            .any(|c| c.status == WorkflowContinuationStatus::Failed)
        {
            let failure = self
                .store
                .fetch_workflow_journal(workflow_run_id)
                .await?
                .into_iter()
                .find_map(|record| match record.entry {
                    WorkflowJournalEntry::Failed { message, .. } => Some(message),
                    _ => None,
                })
                .unwrap_or_else(|| "VM workflow failed".into());
            (WorkflowStatus::Failed, Some(failure))
        } else if deciding
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

fn effect_target(request: &WorkflowEffectRequest) -> Result<ActionTarget, SendableError> {
    let WorkflowEffectRequest::Action {
        required_labels,
        workspace_affinity,
        ..
    } = request
    else {
        return Ok(ActionTarget::Any);
    };
    if required_labels.is_empty() && workspace_affinity.is_none() {
        return Ok(ActionTarget::Any);
    }
    let mut labels = required_labels.clone();
    if let Some(value) = workspace_affinity {
        let affinity: WorkspaceAffinity =
            serde_json::from_value(value.clone().into()).map_err(|error| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid workspace affinity: {error}"),
                )) as SendableError
            })?;
        labels.insert(WORKSPACE_INSTANCE_LABEL.into(), affinity.worker_instance_id);
    }
    Ok(ActionTarget::labels(labels))
}

#[cfg(test)]
mod locality_tests {
    use std::collections::BTreeMap;

    use runinator_models::{
        value::Value, workflow_vm::WorkflowEffectRequest, workflows::WorkflowRetry,
        workspaces::WorkspaceAffinity,
    };

    use super::*;

    #[test]
    fn workspace_affinity_freezes_the_stable_instance_in_the_effect_target() {
        let affinity = WorkspaceAffinity {
            workspace_id: Uuid::now_v7(),
            worker_instance_id: "desktop-a".into(),
            local_key: "admissions/example/source/2-workspace".into(),
            attempt: 2,
            version: 4,
        };
        let request = WorkflowEffectRequest::Action {
            provider: "git".into(),
            function: "run".into(),
            input: Value::Null,
            timeout_seconds: None,
            retry: WorkflowRetry::default(),
            tags: Vec::new(),
            required_labels: BTreeMap::from([("pool".into(), "local".into())]),
            workspace_affinity: Some(Value::from(serde_json::to_value(affinity).unwrap())),
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        };

        let ActionTarget::Labels { selector } = effect_target(&request).unwrap() else {
            panic!("workspace action must be label-targeted")
        };
        assert_eq!(
            selector.get(WORKSPACE_INSTANCE_LABEL),
            Some(&"desktop-a".into())
        );
        assert_eq!(selector.get("pool"), Some(&"local".into()));
    }
}
