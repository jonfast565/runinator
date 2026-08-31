//! Generic correlated-orchestration admission and deterministic intent reduction.

use std::sync::Arc;

use chrono::{Duration, Utc};
use runinator_models::{
    errors::SendableError,
    orchestration::{
        AdapterDefinition, ControlEffect, EpochStopAction, ExternalOperation, IngressAdmission,
        IngressEvent, IngressEventDisposition, IngressEventRecord, IngressInboxEntry,
        IngressLifecycle, IngressPolicy, NewOrchestrationBinding, OrchestrationBinding,
        OrchestrationCommand, OrchestrationCorrelationAlias, OrchestrationEpoch,
        OrchestrationEventReduction, OrchestrationEvidence, OrchestrationPendingIntent,
        OrchestrationPolicy, OrchestrationStatus, RestartSelector, WorkspaceRecovery,
        validate_correlation_alias_identity,
    },
    pipelines::Pipeline,
    value::Value,
};
use runinator_store::roles::{
    DefinitionStore, ExternalOperationUpdate, IngressStore, NewOrchestrationCommand,
    NewOrchestrationCorrelationAlias, NewOrchestrationEpoch, OrchestrationBindingUpdate,
    OrchestrationStore, WorkflowVmStore, WorkspaceStore,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentDecision {
    pub matched: Vec<String>,
    pub winner: Option<String>,
    pub suppressed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentApplyOutcome {
    Applied,
    IgnoredState,
    IgnoredSubjectRevision,
    IgnoredNoActiveMember,
}

impl IntentApplyOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::IgnoredState => "ignored_state",
            Self::IgnoredSubjectRevision => "ignored_subject_revision",
            Self::IgnoredNoActiveMember => "ignored_no_active_member",
        }
    }
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

impl<T: OrchestrationStore> OrchestrationOperations<T> {
    pub async fn list_bindings(
        &self,
        org_id: Option<Uuid>,
        status: Option<OrchestrationStatus>,
        limit: i64,
    ) -> Result<Vec<OrchestrationBinding>, SendableError> {
        self.store
            .fetch_orchestration_bindings(org_id, status, limit)
            .await
    }

    pub async fn fetch_binding(
        &self,
        id: Uuid,
    ) -> Result<Option<OrchestrationBinding>, SendableError> {
        self.store.fetch_orchestration_binding(id).await
    }

    pub async fn epochs(&self, id: Uuid) -> Result<Vec<OrchestrationEpoch>, SendableError> {
        self.store.fetch_orchestration_epochs(id).await
    }

    pub async fn reductions(
        &self,
        id: Uuid,
    ) -> Result<Vec<OrchestrationEventReduction>, SendableError> {
        self.store.fetch_orchestration_reductions(id).await
    }

    pub async fn evidence(&self, id: Uuid) -> Result<Vec<OrchestrationEvidence>, SendableError> {
        self.store.fetch_orchestration_evidence(id).await
    }

    pub async fn commands(&self, id: Uuid) -> Result<Vec<OrchestrationCommand>, SendableError> {
        self.store.fetch_orchestration_commands(id).await
    }

    pub async fn external_operations(
        &self,
        id: Uuid,
    ) -> Result<Vec<ExternalOperation>, SendableError> {
        self.store.fetch_external_operations(id).await
    }

    pub async fn aliases(
        &self,
        id: Uuid,
    ) -> Result<Vec<OrchestrationCorrelationAlias>, SendableError> {
        self.store.fetch_orchestration_correlation_aliases(id).await
    }

    pub async fn add_alias(
        &self,
        binding: &OrchestrationBinding,
        source: String,
        scope: String,
        correlation_key: String,
        now: chrono::DateTime<Utc>,
    ) -> Result<OrchestrationCorrelationAlias, SendableError> {
        validate_correlation_alias_identity(&source, &scope, &correlation_key).map_err(
            |message| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    message,
                )) as SendableError
            },
        )?;
        self.store
            .upsert_orchestration_correlation_alias(
                NewOrchestrationCorrelationAlias {
                    id: Uuid::now_v7(),
                    binding_id: binding.id,
                    generation: binding.generation,
                    org_id: binding.org_id,
                    source,
                    scope,
                    correlation_key,
                },
                now,
            )
            .await
    }

    pub async fn remove_alias(&self, id: Uuid, alias_id: Uuid) -> Result<bool, SendableError> {
        self.store
            .delete_orchestration_correlation_alias(id, alias_id)
            .await
    }

    pub async fn external_operation(
        &self,
        id: Uuid,
    ) -> Result<Option<ExternalOperation>, SendableError> {
        self.store.fetch_external_operation(id).await
    }

    pub async fn update_external_operation(
        &self,
        id: Uuid,
        update: ExternalOperationUpdate,
        now: chrono::DateTime<Utc>,
    ) -> Result<Option<ExternalOperation>, SendableError> {
        self.store.update_external_operation(id, update, now).await
    }

    pub async fn append_evidence(
        &self,
        evidence: OrchestrationEvidence,
    ) -> Result<(), SendableError> {
        self.store.append_orchestration_evidence(evidence).await
    }

    pub async fn adapter(&self, id: Uuid) -> Result<Option<AdapterDefinition>, SendableError> {
        self.store.fetch_orchestration_adapter(id).await
    }
}

impl<T: WorkspaceStore> OrchestrationOperations<T> {
    pub async fn workspaces(
        &self,
        admission_id: Uuid,
        generation: i64,
    ) -> Result<Vec<runinator_models::workspaces::WorkspaceLease>, SendableError> {
        self.store
            .fetch_workspaces_for_admission(admission_id, generation)
            .await
    }
}

impl<T: WorkflowVmStore> OrchestrationOperations<T> {
    pub async fn settle_effect(
        &self,
        effect_id: Uuid,
        attempt: u32,
        status: runinator_models::workflow_vm::WorkflowEffectStatus,
        output: Option<Value>,
        message: Option<String>,
        now: chrono::DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        self.store
            .settle_workflow_effect(effect_id, attempt, status, output, message, now)
            .await
    }

    pub async fn retry_effect(
        &self,
        effect_id: Uuid,
        attempt: u32,
        message: Option<String>,
        now: chrono::DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        self.store
            .retry_workflow_effect(effect_id, attempt, now, message, now)
            .await
    }
}

impl<T: DefinitionStore> OrchestrationOperations<T> {
    pub async fn pipelines(&self) -> Result<Vec<Pipeline>, SendableError> {
        self.store.fetch_pipelines().await
    }
}

impl<T: IngressStore> OrchestrationOperations<T> {
    pub async fn admission(
        &self,
        org_id: Option<Uuid>,
        scope: String,
        correlation_key: String,
    ) -> Result<Option<IngressAdmission>, SendableError> {
        self.store
            .fetch_ingress_admission(org_id, scope, correlation_key)
            .await
    }

    pub async fn record_event(
        &self,
        admission_id: Uuid,
        generation: i64,
        event: IngressEvent,
        disposition: IngressEventDisposition,
        queued: bool,
        now: chrono::DateTime<Utc>,
    ) -> Result<IngressEventRecord, SendableError> {
        self.store
            .record_ingress_event(admission_id, generation, event, disposition, queued, now)
            .await
    }

    pub async fn requeue_event(
        &self,
        admission_id: Uuid,
        expected_generation: i64,
        target: runinator_models::orchestration::IngressTarget,
        policy: Value,
        event: IngressEvent,
        now: chrono::DateTime<Utc>,
    ) -> Result<Option<IngressEventRecord>, SendableError> {
        self.store
            .requeue_ingress_event(
                admission_id,
                expected_generation,
                target,
                policy,
                event,
                now,
            )
            .await
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
        self.admit_with_adapter(admission, pipeline, None).await
    }

    pub async fn admit_with_adapter(
        &self,
        admission: &IngressAdmission,
        pipeline: &Pipeline,
        adapter: Option<(Uuid, i64)>,
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
                adapter_id: adapter.map(|value| value.0),
                adapter_revision: adapter.map(|value| value.1),
                policy,
            })
            .await
            .map(Some)
    }
}

impl<T: OrchestrationStore + IngressStore> OrchestrationOperations<T> {
    /// Put an administrator's emergency low-level run control through the durable inbox. The
    /// control itself remains deliberately out of band, but its immutable event is reduced in
    /// sequence with adapter and operator intents so the timeline cannot hide the bypass.
    #[allow(
        clippy::too_many_arguments,
        reason = "the durable audit event records every override identity and cannot use a lossy context object"
    )]
    pub async fn record_out_of_band_override(
        &self,
        binding: &OrchestrationBinding,
        target_kind: &str,
        target_id: Uuid,
        action: &str,
        reason: String,
        idempotency_key: String,
        actor_id: Option<Uuid>,
    ) -> Result<IngressEventRecord, SendableError> {
        let now = Utc::now();
        self.store
            .record_ingress_event(
                binding.admission_id,
                binding.generation,
                IngressEvent {
                    source: "runinator.admin_override".into(),
                    event_id: idempotency_key,
                    event_type: "out_of_band_override".into(),
                    correlation_key: binding.correlation_key.clone(),
                    payload: runinator_models::json!({
                        "target_kind": target_kind,
                        "target_id": target_id,
                        "action": action,
                        "reason": reason,
                        "actor_id": actor_id,
                    }),
                    provenance: runinator_models::json!({
                        "origin": "platform_admin",
                    }),
                    occurred_at: Some(now),
                },
                IngressEventDisposition::Recorded,
                false,
                now,
            )
            .await
    }

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
        if let Some(pending) = due.into_iter().find(|intent| intent.wake_at <= Utc::now()) {
            (binding, _) = self
                .apply_intent(
                    binding,
                    owner,
                    &pending.intent,
                    pending.latest_payload,
                    None,
                )
                .await?;
            let now = Utc::now();
            let updated = self
                .store
                .consume_orchestration_pending_intent(
                    binding.id,
                    pending.intent,
                    pending.priority,
                    owner.to_string(),
                    OrchestrationBindingUpdate {
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
                        last_reduced_sequence: binding.last_reduced_sequence,
                        finished_at: binding.finished_at,
                    },
                    now,
                )
                .await?;
            binding = updated.ok_or_else(|| {
                Box::new(std::io::Error::other(
                    "coalesced orchestration intent CAS lost",
                )) as SendableError
            })?;
            // Pending intents are sorted by descending policy priority. The atomic consume above
            // removes every lower-priority row, so none from this stale in-memory page may run.
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
        let internal_disposition = self.apply_internal_event(&mut binding, event).await?;
        let self_origin_operation = self
            .self_origin_operation(&binding, &event.provenance)
            .await?;
        let mut decision = IntentDecision {
            matched: Vec::new(),
            winner: None,
            suppressed: Vec::new(),
        };
        let mut disposition = internal_disposition.unwrap_or_else(|| {
            if self_origin_operation.is_some() {
                "self_originated".into()
            } else {
                "observed".into()
            }
        });
        let matching_routes = ingress
            .routes_for_payload(&event.event_type, IngressLifecycle::Active, &event.payload)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let mut intent_outcome = None;
        // Infrastructure lifecycle and platform-admin override events are reducer-owned audit
        // inputs. They must never be reinterpreted as authored dispatch routes: an emergency
        // control has already happened out of band and dispatching it again could double-apply it.
        if !event.source.starts_with("runinator.workspace")
            && event.source != "runinator.admin_override"
        {
            let manual_intent = (event.source == "runinator.manual")
                .then(|| event.payload.get("intent").and_then(Value::as_str))
                .flatten();
            let effective_payload = if manual_intent.is_some() {
                event.payload.get("payload").cloned().unwrap_or_default()
            } else {
                event.payload.clone()
            };
            let routed = matching_routes
                .iter()
                .filter(|route| {
                    route.action == runinator_models::orchestration::IngressAction::Dispatch
                })
                .filter_map(|route| route.intent.as_deref())
                .filter(|name| {
                    self_origin_operation.is_none()
                        || binding
                            .policy
                            .intents
                            .get(*name)
                            .is_some_and(|intent| intent.allow_self_originated)
                });
            decision = choose_intent(manual_intent.into_iter().chain(routed), &binding.policy);
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
                    let wake_at = now + Duration::seconds(seconds as i64);
                    self.store
                        .upsert_orchestration_pending_intent(OrchestrationPendingIntent {
                            id: existing
                                .map(|pending| pending.id)
                                .unwrap_or_else(Uuid::now_v7),
                            binding_id: binding.id,
                            intent: winner.to_string(),
                            priority: intent.priority,
                            source_event_ids,
                            latest_payload: effective_payload,
                            wake_at,
                            created_at: now,
                            updated_at: now,
                        })
                        .await?;
                    self.store
                        .enqueue_orchestration_command(
                            NewOrchestrationCommand {
                                id: Uuid::now_v7(),
                                binding_id: binding.id,
                                epoch: binding.current_epoch,
                                command_type: "arm_intent_wake".into(),
                                operation_key: format!("intent:{winner}:wake:{}", event.id),
                                payload: runinator_models::json!({
                                    "intent": winner,
                                    "wake_at_ms": wake_at.timestamp_millis(),
                                }),
                            },
                            now,
                        )
                        .await?;
                    disposition = if self_origin_operation.is_some() {
                        "self_originated_coalesced".into()
                    } else {
                        "coalesced".into()
                    };
                } else {
                    self.store
                        .delete_orchestration_pending_intents_below(binding.id, intent.priority)
                        .await?;
                    let applied = self
                        .apply_intent(binding, owner, winner, effective_payload, Some(event.id))
                        .await?;
                    binding = applied.0;
                    intent_outcome = Some(applied.1);
                    disposition = if self_origin_operation.is_some() {
                        format!("self_originated_{}", applied.1.as_str())
                    } else {
                        applied.1.as_str().into()
                    };
                }
            }
        }

        if let Some(operation) = &self_origin_operation {
            self.store
                .append_orchestration_evidence(OrchestrationEvidence {
                    id: Uuid::now_v7(),
                    binding_id: binding.id,
                    epoch: Some(binding.current_epoch),
                    kind: "self_originated_event".into(),
                    subject_revision: binding.subject_revision.clone(),
                    payload: runinator_models::json!({
                        "event_id": event.id,
                        "event_type": event.event_type,
                        "payload": event.payload,
                        "provenance": event.provenance,
                        "external_operation_id": operation.id,
                    }),
                    source_event_id: Some(event.id),
                    created_at: Utc::now(),
                })
                .await?;
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
                detail: runinator_models::json!({
                    "event": {
                        "source": event.source,
                        "event_id": event.event_id,
                        "event_type": event.event_type,
                        "correlation_key": event.correlation_key,
                        "occurred_at": event.occurred_at,
                        "received_at": event.received_at,
                        "payload": event.payload,
                    },
                    "matched_routes": matching_routes,
                    "intent_outcome": intent_outcome.map(IntentApplyOutcome::as_str),
                    "self_originated": self_origin_operation.is_some(),
                    "external_operation_id": self_origin_operation.map(|operation| operation.id),
                    "provenance": event.provenance,
                }),
                created_at: now,
            })
            .await?;
        if updated.status.is_terminal() {
            self.store
                .settle_ingress_admission(updated.admission_id, updated.generation, now)
                .await?;
        }
        Ok(updated)
    }

    async fn self_origin_operation(
        &self,
        binding: &OrchestrationBinding,
        provenance: &Value,
    ) -> Result<Option<runinator_models::orchestration::ExternalOperation>, SendableError> {
        let Some(operation_key) = provenance
            .get("operation_key")
            .and_then(Value::as_str)
            .filter(|key| !key.is_empty())
        else {
            return Ok(None);
        };
        Ok(self
            .store
            .fetch_external_operations(binding.id)
            .await?
            .into_iter()
            .find(|operation| {
                operation.operation_key == operation_key
                    || operation
                        .provenance
                        .get("provider_idempotency_key")
                        .and_then(Value::as_str)
                        == Some(operation_key)
            }))
    }

    async fn apply_internal_event(
        &self,
        binding: &mut OrchestrationBinding,
        event: &IngressInboxEntry,
    ) -> Result<Option<String>, SendableError> {
        if event.source != "runinator.workspace" || event.event_type != "workspace_abandoned" {
            return Ok(None);
        }
        let epoch = event.payload.get("epoch").and_then(Value::as_i64);
        let scope = event.payload.get("scope").and_then(Value::as_str);
        self.store
            .append_orchestration_evidence(OrchestrationEvidence {
                id: Uuid::now_v7(),
                binding_id: binding.id,
                epoch,
                kind: "workspace_abandoned".into(),
                subject_revision: binding.subject_revision.clone(),
                payload: event.payload.clone(),
                source_event_id: Some(event.id),
                created_at: Utc::now(),
            })
            .await?;
        if binding.status.is_terminal() || epoch != Some(binding.current_epoch) {
            return Ok(Some("stale_workspace_abandonment".into()));
        }
        let phase = binding
            .current_phase
            .as_deref()
            .and_then(|member| binding.policy.phases.get_key_value(member))
            .filter(|(_, phase)| {
                phase
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| Some(workspace.scope.as_str()) == scope)
            })
            .or_else(|| {
                binding.policy.phases.iter().find(|(_, phase)| {
                    phase
                        .workspace
                        .as_ref()
                        .is_some_and(|workspace| Some(workspace.scope.as_str()) == scope)
                })
            });
        let Some((member, phase)) = phase else {
            binding.status = OrchestrationStatus::Waiting;
            binding.resume_existing_epoch = false;
            return Ok(Some("workspace_policy_missing".into()));
        };
        let member = member.clone();
        let parameters = self
            .store
            .fetch_orchestration_epochs(binding.id)
            .await?
            .into_iter()
            .find(|candidate| candidate.epoch == binding.current_epoch)
            .map(|epoch| epoch.parameters)
            .unwrap_or_else(|| event.payload.clone());
        let recovery = phase
            .workspace
            .as_ref()
            .map(|workspace| workspace.recovery)
            .unwrap_or_default();
        self.enqueue_control(binding, "cancel_epoch", event.payload.clone())
            .await?;
        match recovery {
            WorkspaceRecovery::Replace => {
                let next = binding.current_epoch + 1;
                self.enqueue_epoch(
                    binding,
                    next,
                    Some(member.clone()),
                    parameters,
                    "workspace recovery",
                )
                .await?;
                binding.current_epoch = next;
                binding.status = OrchestrationStatus::Running;
                binding.restart_member = Some(member);
                binding.resume_existing_epoch = false;
                Ok(Some("workspace_replaced".into()))
            }
            WorkspaceRecovery::Wait => {
                binding.status = OrchestrationStatus::Suspended;
                binding.restart_member = Some(member);
                binding.resume_existing_epoch = false;
                Ok(Some("workspace_waiting".into()))
            }
            WorkspaceRecovery::Fail => {
                binding.status = OrchestrationStatus::Failed;
                binding.restart_member = Some(member);
                binding.resume_existing_epoch = false;
                binding.finished_at = Some(Utc::now());
                Ok(Some("workspace_failed".into()))
            }
        }
    }

    async fn apply_intent(
        &self,
        mut binding: OrchestrationBinding,
        owner: &str,
        name: &str,
        payload: Value,
        source_event_id: Option<Uuid>,
    ) -> Result<(OrchestrationBinding, IntentApplyOutcome), SendableError> {
        let intent = binding.policy.intents.get(name).cloned().ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown orchestration intent '{name}'"),
            )) as SendableError
        })?;
        let restart_member = resolve_restart_member(&binding, &intent.restart);
        let mut outcome = IntentApplyOutcome::Applied;
        match intent.effect {
            ControlEffect::Terminate => {
                self.enqueue_control(&binding, "cancel_epoch", payload.clone())
                    .await?;
                binding.status = OrchestrationStatus::Terminated;
                binding.finished_at = Some(Utc::now());
            }
            ControlEffect::Suspend => {
                if binding.status == OrchestrationStatus::Suspended {
                    return Ok((binding, IntentApplyOutcome::IgnoredState));
                }
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
                if matches!(
                    binding.status,
                    OrchestrationStatus::Suspended | OrchestrationStatus::Waiting
                ) {
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
                } else {
                    outcome = IntentApplyOutcome::IgnoredState;
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
                if binding.status != OrchestrationStatus::Running || binding.current_phase.is_none()
                {
                    return Ok((binding, IntentApplyOutcome::IgnoredNoActiveMember));
                }
                if let Some(pointer) = intent.subject_revision_pointer.as_deref()
                    && !signal_revision_matches(
                        binding.subject_revision.as_deref(),
                        &payload,
                        pointer,
                    )
                {
                    return Ok((binding, IntentApplyOutcome::IgnoredSubjectRevision));
                }
                self.enqueue_control(
                    &binding,
                    "signal_epoch",
                    runinator_models::json!({
                        "signal": intent.signal_name.unwrap_or_else(|| name.to_string()),
                        "member": binding.current_phase,
                        "payload": payload,
                    }),
                )
                .await?;
            }
        }
        let _ = owner;
        Ok((binding, outcome))
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

fn signal_revision_matches(expected: Option<&str>, payload: &Value, pointer: &str) -> bool {
    expected
        .is_some_and(|expected| payload.pointer(pointer).and_then(Value::as_str) == Some(expected))
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

    #[test]
    fn revision_bound_signals_require_both_revisions_to_exist_and_match() {
        let payload = runinator_models::json!({ "revision": "r2" });
        assert!(signal_revision_matches(Some("r2"), &payload, "/revision"));
        assert!(!signal_revision_matches(None, &payload, "/revision"));
        assert!(!signal_revision_matches(None, &Value::Null, "/revision"));
        assert!(!signal_revision_matches(Some("r1"), &payload, "/revision"));
        assert!(!signal_revision_matches(Some("r2"), &payload, "/missing"));
    }

    #[tokio::test]
    async fn self_originated_echo_is_evidence_without_dispatch_by_default() {
        use runinator_database::sqlite::SqliteDb;
        use runinator_models::{
            orchestration::{
                DeliverySemantics, ExternalOperation, ExternalOperationStatus, IngressAction,
                IngressAdmissionStatus, IngressEvent, IngressEventDisposition, IngressRoute,
                IngressTarget, IngressTargetKind,
            },
            pipelines::{Pipeline, PipelineGraph},
        };
        use runinator_store::prelude::*;

        let path =
            std::env::temp_dir().join(format!("runinator-self-originated-{}.db", Uuid::now_v7()));
        let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
        db.run_init_scripts(&Vec::new()).await.unwrap();
        let pipeline = db
            .upsert_pipeline(&Pipeline {
                id: None,
                name: "self-origin test".into(),
                key: None,
                namespace: None,
                description: None,
                org_id: None,
                graph: PipelineGraph {
                    version: runinator_models::pipelines::PIPELINE_GRAPH_VERSION,
                    ..Default::default()
                },
                concurrency: Default::default(),
                defaults: Default::default(),
                metadata: Value::Null,
                created_at: None,
                updated_at: None,
            })
            .await
            .unwrap();
        let pipeline_id = pipeline.id.unwrap();
        let ingress = IngressPolicy {
            scope: "objects".into(),
            routes: vec![
                IngressRoute {
                    event_type: "created".into(),
                    lifecycle: IngressLifecycle::Unbound,
                    action: IngressAction::Start,
                    predicates: vec![],
                    intent: None,
                },
                IngressRoute {
                    event_type: "updated".into(),
                    lifecycle: IngressLifecycle::Active,
                    action: IngressAction::Dispatch,
                    predicates: vec![],
                    intent: Some("stop".into()),
                },
                IngressRoute {
                    event_type: "scope_changed".into(),
                    lifecycle: IngressLifecycle::Active,
                    action: IngressAction::Dispatch,
                    predicates: vec![],
                    intent: Some("rework".into()),
                },
                IngressRoute {
                    event_type: "out_of_band_override".into(),
                    lifecycle: IngressLifecycle::Active,
                    action: IngressAction::Dispatch,
                    predicates: vec![],
                    intent: Some("stop".into()),
                },
            ],
        };
        let now = Utc::now();
        let admission = match db
            .claim_ingress_admission(
                IngressAdmission {
                    id: Some(Uuid::now_v7()),
                    org_id: None,
                    scope: ingress.scope.clone(),
                    correlation_key: "object-1".into(),
                    generation: 1,
                    target: IngressTarget {
                        kind: IngressTargetKind::Pipeline,
                        id: pipeline_id,
                    },
                    status: IngressAdmissionStatus::Active,
                    workflow_run_id: None,
                    pipeline_run_id: None,
                    policy: serde_json::to_value(&ingress).unwrap().into(),
                    created_at: now,
                    updated_at: now,
                },
                Some(IngressEvent {
                    source: "adapter:test".into(),
                    event_id: "created".into(),
                    event_type: "created".into(),
                    correlation_key: "object-1".into(),
                    payload: Value::Null,
                    provenance: Value::Null,
                    occurred_at: Some(now),
                }),
            )
            .await
            .unwrap()
        {
            runinator_models::orchestration::IngressAdmissionClaim::Acquired(value) => value,
            _ => panic!("admission must be acquired"),
        };
        let mut policy = OrchestrationPolicy::default();
        policy.intents.insert(
            "stop".into(),
            IntentPolicy {
                effect: ControlEffect::Terminate,
                priority: 100,
                coalesce_seconds: None,
                stop: Default::default(),
                restart: Default::default(),
                subject_revision_pointer: None,
                allow_self_originated: false,
                signal_name: None,
            },
        );
        policy.intents.insert(
            "rework".into(),
            IntentPolicy {
                effect: ControlEffect::Supersede,
                priority: 80,
                coalesce_seconds: Some(300),
                stop: Default::default(),
                restart: Default::default(),
                subject_revision_pointer: None,
                allow_self_originated: false,
                signal_name: None,
            },
        );
        let binding = db
            .create_orchestration_binding(NewOrchestrationBinding {
                id: Uuid::now_v7(),
                admission_id: admission.id.unwrap(),
                org_id: None,
                scope: admission.scope.clone(),
                correlation_key: admission.correlation_key.clone(),
                generation: 1,
                pipeline_id,
                pipeline_revision: 1,
                pipeline_digest: "test".into(),
                adapter_id: None,
                adapter_revision: None,
                policy,
            })
            .await
            .unwrap();
        db.create_external_operation(ExternalOperation {
            id: Uuid::now_v7(),
            binding_id: binding.id,
            epoch: 1,
            workflow_run_id: None,
            effect_id: None,
            operation_key: "effect-key".into(),
            provider: "example".into(),
            action: "ensure".into(),
            semantics: DeliverySemantics::Reconcilable,
            attempt: 1,
            status: ExternalOperationStatus::Succeeded,
            ambiguous: false,
            provenance: runinator_models::json!({
                "provider_idempotency_key": "provider-key"
            }),
            receipt: Value::Null,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
        db.record_ingress_event(
            binding.admission_id,
            binding.generation,
            IngressEvent {
                source: "adapter:test".into(),
                event_id: "echo".into(),
                event_type: "updated".into(),
                correlation_key: binding.correlation_key.clone(),
                payload: Value::Null,
                provenance: runinator_models::json!({ "operation_key": "provider-key" }),
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
        let claimed = db
            .claim_orchestration_bindings(
                "self-origin-reducer".into(),
                now,
                now + Duration::minutes(1),
                1,
            )
            .await
            .unwrap()
            .pop()
            .unwrap();
        let reduced = OrchestrationOperations::new(db.clone())
            .reduce_binding(claimed, "self-origin-reducer")
            .await
            .unwrap();
        assert_eq!(reduced.status, OrchestrationStatus::Running);
        let reductions = db.fetch_orchestration_reductions(binding.id).await.unwrap();
        assert_eq!(reductions.last().unwrap().disposition, "self_originated");
        assert!(reductions.last().unwrap().winner.is_none());
        let evidence = db.fetch_orchestration_evidence(binding.id).await.unwrap();
        assert!(
            evidence
                .iter()
                .any(|item| item.kind == "self_originated_event")
        );
        assert_eq!(
            reductions
                .last()
                .unwrap()
                .detail
                .pointer("/event/event_id")
                .and_then(Value::as_str),
            Some("echo")
        );
        assert_eq!(
            reductions
                .last()
                .unwrap()
                .detail
                .pointer("/matched_routes/0/intent")
                .and_then(Value::as_str),
            Some("stop")
        );

        db.record_ingress_event(
            binding.admission_id,
            binding.generation,
            IngressEvent {
                source: "adapter:test".into(),
                event_id: "scope-change".into(),
                event_type: "scope_changed".into(),
                correlation_key: binding.correlation_key.clone(),
                payload: runinator_models::json!({ "revision": "r2" }),
                provenance: Value::Null,
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
        let reduced = OrchestrationOperations::new(db.clone())
            .reduce_binding(reduced, "self-origin-reducer")
            .await
            .unwrap();
        let pending = db
            .fetch_orchestration_pending_intents(binding.id)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].intent, "rework");
        let commands = db.fetch_orchestration_commands(binding.id).await.unwrap();
        let wake = commands
            .iter()
            .find(|command| command.command_type == "arm_intent_wake")
            .expect("coalescing should arm the broker waker through the command outbox");
        assert_eq!(
            wake.payload.get("intent").and_then(Value::as_str),
            Some("rework")
        );

        db.record_ingress_event(
            binding.admission_id,
            binding.generation,
            IngressEvent {
                source: "adapter:test".into(),
                event_id: "scope-change-newer".into(),
                event_type: "scope_changed".into(),
                correlation_key: binding.correlation_key.clone(),
                payload: runinator_models::json!({ "revision": "r3" }),
                provenance: Value::Null,
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
        let reduced = OrchestrationOperations::new(db.clone())
            .reduce_binding(reduced, "self-origin-reducer")
            .await
            .unwrap();
        let pending = db
            .fetch_orchestration_pending_intents(binding.id)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].source_event_ids.len(), 2);
        assert_eq!(
            pending[0]
                .latest_payload
                .get("revision")
                .and_then(Value::as_str),
            Some("r3")
        );
        assert_eq!(
            db.fetch_orchestration_commands(binding.id)
                .await
                .unwrap()
                .iter()
                .filter(|command| command.command_type == "arm_intent_wake")
                .count(),
            2
        );

        let operations = OrchestrationOperations::new(db.clone());
        let override_record = operations
            .record_out_of_band_override(
                &reduced,
                "pipeline_run",
                Uuid::now_v7(),
                "pause",
                "recover inconsistent provider state".into(),
                "admin-override-1".into(),
                Some(Uuid::now_v7()),
            )
            .await
            .unwrap();
        assert!(!override_record.duplicate);
        let duplicate = operations
            .record_out_of_band_override(
                &reduced,
                "pipeline_run",
                Uuid::now_v7(),
                "pause",
                "retry of the same request".into(),
                "admin-override-1".into(),
                Some(Uuid::now_v7()),
            )
            .await
            .unwrap();
        assert!(duplicate.duplicate);
        let reduced = operations
            .reduce_binding(reduced, "self-origin-reducer")
            .await
            .unwrap();
        let reductions = db.fetch_orchestration_reductions(binding.id).await.unwrap();
        let override_reduction = reductions
            .iter()
            .find(|reduction| {
                reduction
                    .detail
                    .pointer("/event/event_type")
                    .and_then(Value::as_str)
                    == Some("out_of_band_override")
            })
            .expect("administrator override should pass through the reducer timeline");
        assert_eq!(override_reduction.disposition, "observed");
        assert_eq!(
            override_reduction
                .detail
                .pointer("/event/payload/action")
                .and_then(Value::as_str),
            Some("pause")
        );

        db.record_ingress_event(
            binding.admission_id,
            binding.generation,
            IngressEvent {
                source: "runinator.manual".into(),
                event_id: "manual-stop".into(),
                event_type: "manual_intent".into(),
                correlation_key: binding.correlation_key.clone(),
                payload: runinator_models::json!({
                    "intent": "stop",
                    "payload": { "reason": "operator request" },
                    "reason": "audit reason",
                }),
                provenance: Value::Null,
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
        let reduced = OrchestrationOperations::new(db.clone())
            .reduce_binding(reduced, "self-origin-reducer")
            .await
            .unwrap();
        assert_eq!(reduced.status, OrchestrationStatus::Terminated);
        assert!(
            db.fetch_orchestration_pending_intents(binding.id)
                .await
                .unwrap()
                .is_empty()
        );
        let commands = db.fetch_orchestration_commands(binding.id).await.unwrap();
        assert_eq!(
            commands
                .iter()
                .find(|command| command.command_type == "cancel_epoch")
                .map(|command| command.payload.clone()),
            Some(runinator_models::json!({ "reason": "operator request" }))
        );
        let reductions = db.fetch_orchestration_reductions(binding.id).await.unwrap();
        assert_eq!(reductions.last().unwrap().disposition, "applied");

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
