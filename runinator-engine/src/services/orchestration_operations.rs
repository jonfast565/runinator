//! Generic correlated-orchestration admission and deterministic intent reduction.

use std::sync::Arc;

use chrono::{Duration, Utc};
use runinator_models::{
    errors::SendableError,
    orchestration::{
        ControlEffect, EpochStopAction, IngressAdmission, IngressInboxEntry, IngressLifecycle,
        IngressPolicy, NewOrchestrationBinding, OrchestrationBinding, OrchestrationEventReduction,
        OrchestrationEvidence, OrchestrationPendingIntent, OrchestrationPolicy,
        OrchestrationStatus, RestartSelector,
    },
    pipelines::Pipeline,
    value::Value,
};
use runinator_store::roles::{
    DefinitionStore, IngressStore, NewOrchestrationCommand, NewOrchestrationEpoch,
    OrchestrationBindingUpdate, OrchestrationStore,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentDecision {
    pub matched: Vec<String>,
    pub winner: Option<String>,
    pub suppressed: Vec<String>,
}

/// Resolve named-intent precedence without knowing any provider or problem-domain vocabulary.
pub fn choose_intent<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    policy: &OrchestrationPolicy,
) -> IntentDecision {
    let mut matched = candidates
        .into_iter()
        .filter(|name| policy.intents.contains_key(*name))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    matched.sort();
    matched.dedup();
    matched.sort_by(|left, right| {
        policy.intents[right]
            .priority
            .cmp(&policy.intents[left].priority)
            .then_with(|| left.cmp(right))
    });
    let winner = matched.first().cloned();
    let suppressed = matched.iter().skip(1).cloned().collect();
    IntentDecision {
        matched,
        winner,
        suppressed,
    }
}

#[derive(Clone)]
pub struct OrchestrationOperations<T> {
    store: Arc<T>,
}

impl<T> OrchestrationOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: OrchestrationStore + DefinitionStore> OrchestrationOperations<T> {
    /// Create the durable binding snapshot for a managed pipeline admission. The first reducer pass
    /// creates epoch one through the same command-outbox path used by every later restart.
    pub async fn admit(
        &self,
        admission: &IngressAdmission,
        pipeline: &Pipeline,
    ) -> Result<Option<OrchestrationBinding>, SendableError> {
        let Some(policy_value) = pipeline.metadata.get("orchestration") else {
            return Ok(None);
        };
        let policy: OrchestrationPolicy = serde_json::from_value(policy_value.clone().into())?;
        policy
            .validate(
                pipeline
                    .graph
                    .members
                    .iter()
                    .map(|member| member.key.as_str()),
            )
            .map_err(|message| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    message,
                )) as SendableError
            })?;
        let pipeline_id = pipeline.id.ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "managed pipeline is missing an id",
            )) as SendableError
        })?;
        let revision = self
            .store
            .fetch_pipeline_revisions(pipeline_id, 1)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                Box::new(std::io::Error::other(
                    "managed pipeline has no immutable revision",
                )) as SendableError
            })?;
        let admission_id = admission.id.ok_or_else(|| {
            Box::new(std::io::Error::other("stored admission is missing an id")) as SendableError
        })?;
        self.store
            .create_orchestration_binding(NewOrchestrationBinding {
                id: Uuid::now_v7(),
                admission_id,
                org_id: admission.org_id,
                scope: admission.scope.clone(),
                correlation_key: admission.correlation_key.clone(),
                generation: admission.generation,
                pipeline_id,
                pipeline_revision: revision.revision,
                pipeline_digest: revision.digest,
                policy,
            })
            .await
            .map(Some)
    }
}

impl<T: OrchestrationStore + IngressStore> OrchestrationOperations<T> {
    pub async fn reduce_binding(
        &self,
        mut binding: OrchestrationBinding,
        owner: &str,
    ) -> Result<OrchestrationBinding, SendableError> {
        let admission = self
            .store
            .fetch_ingress_admission(
                binding.org_id,
                binding.scope.clone(),
                binding.correlation_key.clone(),
            )
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other("orchestration admission disappeared"))
                    as SendableError
            })?;
        let ingress: IngressPolicy = serde_json::from_value(admission.policy.clone().into())?;
        let events = self
            .store
            .fetch_ingress_events(binding.admission_id)
            .await?;

        let last_reduced_sequence = binding.last_reduced_sequence;
        for event in events
            .into_iter()
            .filter(|event| event.sequence > last_reduced_sequence)
        {
            binding = self.reduce_event(binding, owner, &ingress, &event).await?;
        }

        let due = self
            .store
            .fetch_orchestration_pending_intents(binding.id)
            .await?;
        for pending in due
            .into_iter()
            .filter(|intent| intent.wake_at <= Utc::now())
        {
            binding = self
                .apply_intent(
                    binding,
                    owner,
                    &pending.intent,
                    pending.latest_payload,
                    None,
                )
                .await?;
            self.store
                .delete_orchestration_pending_intent(binding.id, pending.intent)
                .await?;
        }
        Ok(binding)
    }

    async fn reduce_event(
        &self,
        mut binding: OrchestrationBinding,
        owner: &str,
        ingress: &IngressPolicy,
        event: &IngressInboxEntry,
    ) -> Result<OrchestrationBinding, SendableError> {
        if binding.current_epoch == 0 {
            self.enqueue_epoch(
                &binding,
                1,
                None,
                event.payload.clone(),
                "initial admission",
            )
            .await?;
            binding.current_epoch = 1;
            binding.status = OrchestrationStatus::Running;
        }
        let manual_intent = (event.source == "runinator.manual")
            .then(|| event.payload.get("intent").and_then(Value::as_str))
            .flatten();
        let routed =
            ingress.dispatches_for(&event.event_type, IngressLifecycle::Active, &event.payload);
        let decision = choose_intent(manual_intent.into_iter().chain(routed), &binding.policy);
        let mut disposition = "observed".to_string();
        if let Some(winner) = decision.winner.as_deref() {
            let intent = binding.policy.intents[winner].clone();
            if let Some(seconds) = intent.coalesce_seconds {
                let existing = self
                    .store
                    .fetch_orchestration_pending_intents(binding.id)
                    .await?
                    .into_iter()
                    .find(|pending| pending.intent == winner);
                let mut source_event_ids = existing
                    .as_ref()
                    .map(|pending| pending.source_event_ids.clone())
                    .unwrap_or_default();
                if !source_event_ids.contains(&event.id) {
                    source_event_ids.push(event.id);
                }
                let now = Utc::now();
                self.store
                    .upsert_orchestration_pending_intent(OrchestrationPendingIntent {
                        id: existing
                            .map(|pending| pending.id)
                            .unwrap_or_else(Uuid::now_v7),
                        binding_id: binding.id,
                        intent: winner.to_string(),
                        priority: intent.priority,
                        source_event_ids,
                        latest_payload: event.payload.clone(),
                        wake_at: now + Duration::seconds(seconds as i64),
                        created_at: now,
                        updated_at: now,
                    })
                    .await?;
                disposition = "coalesced".into();
            } else {
                self.store
                    .delete_orchestration_pending_intents_below(binding.id, intent.priority)
                    .await?;
                binding = self
                    .apply_intent(
                        binding,
                        owner,
                        winner,
                        event.payload.clone(),
                        Some(event.id),
                    )
                    .await?;
                disposition = "applied".into();
            }
        }

        let now = Utc::now();
        let update = OrchestrationBindingUpdate {
            expected_version: binding.version,
            status: binding.status,
            current_phase: binding.current_phase.clone(),
            current_attempt: binding.current_attempt,
            current_epoch: binding.current_epoch,
            restart_member: binding.restart_member.clone(),
            resume_existing_epoch: binding.resume_existing_epoch,
            subject_revision: binding.subject_revision.clone(),
            resources: binding.resources.clone(),
            budgets: binding.budgets.clone(),
            last_reduced_sequence: event.sequence,
            finished_at: binding.finished_at,
        };
        let updated = self
            .store
            .update_orchestration_binding(binding.id, owner.to_string(), update, now)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other("orchestration binding CAS lost")) as SendableError
            })?;
        self.store
            .record_orchestration_reduction(OrchestrationEventReduction {
                id: Uuid::now_v7(),
                binding_id: binding.id,
                inbox_event_id: event.id,
                sequence: event.sequence,
                matched_intents: decision.matched,
                winner: decision.winner,
                suppressed_intents: decision.suppressed,
                binding_version: updated.version,
                disposition,
                detail: Value::Null,
                created_at: now,
            })
            .await?;
        Ok(updated)
    }

    async fn apply_intent(
        &self,
        mut binding: OrchestrationBinding,
        owner: &str,
        name: &str,
        payload: Value,
        source_event_id: Option<Uuid>,
    ) -> Result<OrchestrationBinding, SendableError> {
        let intent = binding.policy.intents.get(name).cloned().ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown orchestration intent '{name}'"),
            )) as SendableError
        })?;
        let restart_member = resolve_restart_member(&binding, &intent.restart);
        match intent.effect {
            ControlEffect::Terminate => {
                self.enqueue_control(&binding, "cancel_epoch", payload.clone())
                    .await?;
                binding.status = OrchestrationStatus::Terminated;
                binding.finished_at = Some(Utc::now());
            }
            ControlEffect::Suspend => {
                let command = match intent.stop {
                    EpochStopAction::Pause => "pause_epoch",
                    EpochStopAction::Cancel => "cancel_epoch",
                    EpochStopAction::None => "observe_epoch",
                };
                if command != "observe_epoch" {
                    self.enqueue_control(&binding, command, payload.clone())
                        .await?;
                }
                binding.status = OrchestrationStatus::Suspended;
                binding.restart_member = restart_member;
                binding.resume_existing_epoch = intent.stop == EpochStopAction::Pause;
            }
            ControlEffect::Resume => {
                if binding.status == OrchestrationStatus::Suspended {
                    if binding.resume_existing_epoch {
                        self.enqueue_control(&binding, "resume_epoch", payload.clone())
                            .await?;
                    } else {
                        let next = binding.current_epoch + 1;
                        self.enqueue_epoch(
                            &binding,
                            next,
                            binding.restart_member.clone(),
                            payload.clone(),
                            name,
                        )
                        .await?;
                        binding.current_epoch = next;
                    }
                    binding.status = OrchestrationStatus::Running;
                    binding.restart_member = None;
                    binding.resume_existing_epoch = false;
                }
            }
            ControlEffect::Supersede => {
                self.enqueue_control(&binding, "cancel_epoch", payload.clone())
                    .await?;
                let next = binding.current_epoch + 1;
                self.enqueue_epoch(&binding, next, restart_member, payload.clone(), name)
                    .await?;
                binding.current_epoch = next;
                binding.status = OrchestrationStatus::Running;
            }
            ControlEffect::Observe => {
                self.store
                    .append_orchestration_evidence(OrchestrationEvidence {
                        id: Uuid::now_v7(),
                        binding_id: binding.id,
                        epoch: Some(binding.current_epoch),
                        kind: name.to_string(),
                        subject_revision: binding.subject_revision.clone(),
                        payload,
                        source_event_id,
                        created_at: Utc::now(),
                    })
                    .await?;
            }
            ControlEffect::Signal => {
                if let Some(pointer) = intent.subject_revision_pointer.as_deref()
                    && payload.pointer(pointer).and_then(Value::as_str)
                        != binding.subject_revision.as_deref()
                {
                    return Ok(binding);
                }
                self.enqueue_control(
                    &binding,
                    "signal_epoch",
                    runinator_models::json!({
                        "signal": intent.signal_name.unwrap_or_else(|| name.to_string()),
                        "payload": payload,
                    }),
                )
                .await?;
            }
        }
        let _ = owner;
        Ok(binding)
    }

    async fn enqueue_epoch(
        &self,
        binding: &OrchestrationBinding,
        epoch: i64,
        start_member: Option<String>,
        parameters: Value,
        reason: &str,
    ) -> Result<(), SendableError> {
        self.store
            .create_orchestration_epoch(
                NewOrchestrationEpoch {
                    id: Uuid::now_v7(),
                    binding_id: binding.id,
                    epoch,
                    start_member: start_member.clone(),
                    parameters: parameters.clone(),
                    reason: reason.to_string(),
                },
                Utc::now(),
            )
            .await?;
        self.store.enqueue_orchestration_command(NewOrchestrationCommand {
            id: Uuid::now_v7(), binding_id: binding.id, epoch, command_type: "start_epoch".into(),
            operation_key: format!("epoch:{epoch}:start"),
            payload: runinator_models::json!({ "parameters": parameters, "start_member": start_member }),
        }, Utc::now()).await?;
        Ok(())
    }

    async fn enqueue_control(
        &self,
        binding: &OrchestrationBinding,
        command_type: &str,
        payload: Value,
    ) -> Result<(), SendableError> {
        self.store
            .enqueue_orchestration_command(
                NewOrchestrationCommand {
                    id: Uuid::now_v7(),
                    binding_id: binding.id,
                    epoch: binding.current_epoch,
                    command_type: command_type.to_string(),
                    operation_key: format!(
                        "epoch:{}:{command_type}:v{}",
                        binding.current_epoch, binding.version
                    ),
                    payload,
                },
                Utc::now(),
            )
            .await?;
        Ok(())
    }
}

fn resolve_restart_member(
    binding: &OrchestrationBinding,
    selector: &RestartSelector,
) -> Option<String> {
    match selector {
        RestartSelector::Entry => None,
        RestartSelector::Current => binding.current_phase.clone(),
        RestartSelector::Member(member) => Some(member.clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use runinator_models::orchestration::{IntentPolicy, OrchestrationPolicy};

    #[test]
    fn precedence_is_priority_driven_and_suppressed_is_stable() {
        let mut intents = BTreeMap::new();
        for (name, effect, priority) in [
            ("audit", ControlEffect::Observe, 10),
            ("redo", ControlEffect::Supersede, 80),
            ("stop", ControlEffect::Terminate, 100),
        ] {
            intents.insert(
                name.into(),
                IntentPolicy {
                    effect,
                    priority,
                    coalesce_seconds: None,
                    stop: Default::default(),
                    restart: Default::default(),
                    subject_revision_pointer: None,
                    allow_self_originated: false,
                    signal_name: None,
                },
            );
        }
        let policy = OrchestrationPolicy {
            intents,
            ..Default::default()
        };
        assert_eq!(
            choose_intent(["audit", "stop", "redo"], &policy),
            IntentDecision {
                matched: vec!["stop".into(), "redo".into(), "audit".into()],
                winner: Some("stop".into()),
                suppressed: vec!["redo".into(), "audit".into()],
            }
        );
    }
}
