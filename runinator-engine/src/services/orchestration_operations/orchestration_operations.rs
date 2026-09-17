#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub struct OrchestrationOperations<T> {
    pub(super) store: Arc<T>,
}

impl<T> OrchestrationOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: OrchestrationStore> OrchestrationOperations<T> {
    pub async fn debug_control(
        &self,
        id: Uuid,
    ) -> Result<runinator_models::adapter_control::OrchestrationDebugControl, SendableError> {
        self.store.orchestration_debug_control(id).await
    }
    pub async fn set_debug_control(
        &self,
        id: Uuid,
        value: runinator_models::adapter_control::OrchestrationDebugControl,
    ) -> Result<(), SendableError> {
        self.store.set_orchestration_debug_control(id, value).await
    }
    pub async fn list_bindings(
        &self,
        org_id: Option<Uuid>,
        filter: OrchestrationBindingFilter,
    ) -> Result<Vec<OrchestrationBinding>, SendableError> {
        self.store
            .fetch_orchestration_bindings(org_id, filter)
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

impl<T: OrchestrationStore + RuntimeStore + WorkflowVmStore> OrchestrationOperations<T> {
    /// Resolve the one active steerable AI effect owned by the binding's current epoch.
    pub async fn active_mission_harness_effect(
        &self,
        binding: &runinator_models::orchestration::OrchestrationBinding,
    ) -> Result<Option<runinator_models::workflow_vm::WorkflowEffect>, SendableError> {
        if !binding.scope.starts_with("mission.") || binding.status.is_terminal() {
            return Ok(None);
        }
        let Some(pipeline_run_id) = self
            .store
            .fetch_orchestration_epochs(binding.id)
            .await?
            .into_iter()
            .find(|epoch| epoch.epoch == binding.current_epoch)
            .and_then(|epoch| epoch.pipeline_run_id)
        else {
            return Ok(None);
        };
        let mut attempts = self
            .store
            .fetch_pipeline_member_attempts(pipeline_run_id)
            .await?;
        attempts.sort_by_key(|attempt| std::cmp::Reverse(attempt.attempt));
        for workflow_run_id in attempts
            .into_iter()
            .filter(|attempt| {
                !attempt.status.is_terminal()
                    && binding.current_phase.as_deref() == Some(attempt.member_key.as_str())
            })
            .filter_map(|attempt| attempt.workflow_run_id)
        {
            let effect = self
                .store
                .fetch_workflow_effects(workflow_run_id)
                .await?
                .into_iter()
                .find(is_active_harnessed_ai_effect);
            if effect.is_some() {
                return Ok(effect);
            }
        }
        Ok(None)
    }

    /// Resolve the current unresolved approval owned by the binding's active mission phase.
    pub async fn active_mission_approval_effect(
        &self,
        binding: &runinator_models::orchestration::OrchestrationBinding,
    ) -> Result<Option<runinator_models::workflow_vm::WorkflowEffect>, SendableError> {
        if !binding.scope.starts_with("mission.") || binding.status.is_terminal() {
            return Ok(None);
        }
        let Some(pipeline_run_id) = self
            .store
            .fetch_orchestration_epochs(binding.id)
            .await?
            .into_iter()
            .find(|epoch| epoch.epoch == binding.current_epoch)
            .and_then(|epoch| epoch.pipeline_run_id)
        else {
            return Ok(None);
        };
        let mut attempts = self
            .store
            .fetch_pipeline_member_attempts(pipeline_run_id)
            .await?;
        attempts.sort_by_key(|attempt| std::cmp::Reverse(attempt.attempt));
        for workflow_run_id in attempts
            .into_iter()
            .filter(|attempt| {
                !attempt.status.is_terminal()
                    && binding.current_phase.as_deref() == Some(attempt.member_key.as_str())
            })
            .filter_map(|attempt| attempt.workflow_run_id)
        {
            let effect = self
                .store
                .fetch_workflow_effects(workflow_run_id)
                .await?
                .into_iter()
                .find(|effect| {
                    !effect.status.is_terminal()
                        && matches!(
                            effect.request,
                            runinator_models::workflow_vm::WorkflowEffectRequest::Approval { .. }
                        )
                });
            if effect.is_some() {
                return Ok(effect);
            }
        }
        Ok(None)
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
    pub async fn record_out_of_band_override(
        &self,
        binding: &OrchestrationBinding,
        request: OutOfBandOverrideRequest,
    ) -> Result<IngressEventRecord, SendableError> {
        let now = Utc::now();
        self.store
            .record_ingress_event(
                binding.admission_id,
                binding.generation,
                IngressEvent {
                    source: "runinator.admin_override".into(),
                    event_id: request.idempotency_key,
                    event_type: "out_of_band_override".into(),
                    correlation_key: binding.correlation_key.clone(),
                    payload: runinator_models::json!({
                        "target_kind": request.target_kind,
                        "target_id": request.target_id,
                        "action": request.action,
                        "reason": request.reason,
                        "actor_id": request.actor_id,
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
            if !self
                .store
                .take_orchestration_debug_permit(binding.pipeline_id)
                .await?
            {
                return Ok(binding);
            }
            binding = self.reduce_event(binding, owner, &ingress, &event).await?;
        }

        let due = self
            .store
            .fetch_orchestration_pending_intents(binding.id)
            .await?;
        if let Some(pending) = due.into_iter().find(|intent| intent.wake_at <= Utc::now()) {
            if !self
                .store
                .take_orchestration_debug_permit(binding.pipeline_id)
                .await?
            {
                return Ok(binding);
            }
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

    pub(super) async fn reduce_event(
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
                binding.policy.entry_member.clone(),
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

    pub(super) async fn self_origin_operation(
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

    pub(super) async fn apply_internal_event(
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

    pub(super) async fn apply_intent(
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

    pub(super) async fn enqueue_epoch(
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

    pub(super) async fn enqueue_control(
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
