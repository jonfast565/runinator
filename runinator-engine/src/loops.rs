use std::collections::BTreeMap;
use std::{sync::Arc, time::Duration};

use runinator_broker_core::{Broker, WakeMessage};
use runinator_comm::{AgentDirectiveKind, AgentDirectiveState, WakeCommand};
use runinator_models::errors::error_code_or_unknown;
use runinator_models::replicas::{ReplicaKind, ReplicaStatus};
use runinator_models::{
    interrupt::InterruptSource,
    orchestration::{
        BudgetExhaustion, DeliverySemantics, ExternalOperation, ExternalOperationStatus,
        IngressPromotion, IngressTargetKind, OrchestrationEvidence, OrchestrationStatus,
        validate_correlation_alias_identity,
    },
    pipelines::{PipelineExecutionContext, PipelineMemberAttempt, PipelineMemberAttemptStatus},
    replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance},
    value::Value,
    workflow_vm::{WorkflowEffectRequest, WorkflowEffectStatus},
    workspaces::WorkspaceAffinity,
};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, ExternalOperationUpdate, IngressStore, NewOrchestrationCommand,
        NewOrchestrationCorrelationAlias, NewOrchestrationEpoch, NotificationStore,
        OrchestrationBindingUpdate, OrchestrationStore, OrgStore, ReplicaStore, ScheduleStore,
        WorkflowVmStore, WorkspaceStore,
    },
};

/// Reduce correlated bindings and execute their durable command outbox. All provider-specific
/// interpretation has already happened at the adapter edge; this loop only sees named intents and
/// the engine's closed control-effect vocabulary.
pub async fn run_correlated_orchestration_reducer<
    T: RuntimeStore
        + WorkflowVmStore
        + DefinitionStore
        + IngressStore
        + OrchestrationStore
        + WorkspaceStore
        + ReplicaStore,
>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    publisher: crate::events::EventSender,
    instance: String,
    nudge: Arc<Notify>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!(instance = %instance, "correlated orchestration reducer started");
    let service = crate::services::OrchestrationOperations::new(db.clone());
    loop {
        let policy = settings.current();
        let now = chrono::Utc::now();
        let lease = now
            + chrono::Duration::seconds(
                policy.orchestration.correlated_reducer_lease_seconds as i64,
            );
        match db
            .claim_orchestration_bindings(
                instance.clone(),
                now,
                lease,
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(bindings) => {
                for binding in bindings {
                    let binding_id = binding.id;
                    let org_id = binding.org_id;
                    match service.reduce_binding(binding, &instance).await {
                        Ok(binding) => {
                            if let Err(err) =
                                settle_current_orchestration_epoch(db.as_ref(), &instance, &binding)
                                    .await
                            {
                                warn!(%binding_id, error = %err, "failed to settle orchestration epoch");
                            }
                        }
                        Err(err) => {
                            warn!(%binding_id, error = %err, "failed to reduce orchestration binding")
                        }
                    }
                    crate::events::emit_orchestration(&publisher, binding_id, org_id);
                    if let Err(err) = db
                        .release_orchestration_binding_lease(binding_id, instance.clone())
                        .await
                    {
                        warn!(%binding_id, error = %err, "failed to release orchestration reducer lease");
                    }
                }
            }
            Err(err) => warn!(error = %err, "failed to claim orchestration bindings"),
        }

        match db
            .claim_orchestration_commands(
                instance.clone(),
                now,
                lease,
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(commands) => {
                for command in commands {
                    let outcome =
                        execute_orchestration_command(db.clone(), broker.as_ref(), &command).await;
                    let (succeeded, result) = match outcome {
                        Ok(result) => (true, result),
                        Err(err) => {
                            warn!(command_id = %command.id, error = %err, "orchestration command failed");
                            (false, runinator_models::json!({ "error": err.to_string() }))
                        }
                    };
                    let now = chrono::Utc::now();
                    let settle = if succeeded {
                        db.complete_orchestration_command(
                            command.id,
                            instance.clone(),
                            true,
                            result.clone(),
                            now,
                        )
                        .await
                    } else {
                        // Internal commands are operation-keyed and replay-safe. Keep retrying the
                        // durable row instead of failing it and stranding a running binding.
                        db.retry_orchestration_command(
                            command.id,
                            instance.clone(),
                            result.clone(),
                            now,
                        )
                        .await
                    };
                    if let Err(err) = settle {
                        warn!(command_id = %command.id, error = %err, "failed to settle orchestration command");
                    }
                    if let Ok(Some(binding)) =
                        db.fetch_orchestration_binding(command.binding_id).await
                    {
                        crate::events::emit_orchestration(&publisher, binding.id, binding.org_id);
                    }
                }
            }
            Err(err) => warn!(error = %err, "failed to claim orchestration commands"),
        }

        tokio::select! {
            _ = shutdown.notified() => return,
            _ = nudge.notified() => {}
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.correlated_reducer_poll_interval_ms)) => {}
        }
    }
}

async fn execute_orchestration_command<
    T: RuntimeStore
        + WorkflowVmStore
        + DefinitionStore
        + IngressStore
        + OrchestrationStore
        + WorkspaceStore
        + ReplicaStore,
>(
    db: Arc<T>,
    broker: &dyn Broker,
    command: &runinator_models::orchestration::OrchestrationCommand,
) -> Result<runinator_models::value::Value, runinator_models::errors::SendableError> {
    let binding = db
        .fetch_orchestration_binding(command.binding_id)
        .await?
        .ok_or_else(|| {
            Box::new(std::io::Error::other(
                "orchestration command binding disappeared",
            )) as runinator_models::errors::SendableError
        })?;
    match orchestration_command_fence(
        binding.current_epoch,
        binding.status,
        command.epoch,
        &command.command_type,
    ) {
        OrchestrationCommandFence::Execute => {}
        OrchestrationCommandFence::Retry => {
            return Err(Box::new(std::io::Error::other(format!(
                "orchestration binding has not committed epoch {} yet",
                command.epoch
            ))));
        }
        OrchestrationCommandFence::Stale(reason) => {
            return record_stale_orchestration_command(db.as_ref(), &binding, command, reason)
                .await;
        }
    }
    let epoch = db
        .fetch_orchestration_epochs(binding.id)
        .await?
        .into_iter()
        .find(|epoch| epoch.epoch == command.epoch)
        .ok_or_else(|| {
            Box::new(std::io::Error::other(
                "orchestration command epoch disappeared",
            )) as runinator_models::errors::SendableError
        })?;
    match command.command_type.as_str() {
        "start_epoch" => {
            if let Some(run_id) = epoch.pipeline_run_id {
                return Ok(
                    runinator_models::json!({ "pipeline_run_id": run_id, "replayed": true }),
                );
            }
            let parameters = command
                .payload
                .get("parameters")
                .cloned()
                .unwrap_or_default();
            let parameters =
                prepare_epoch_workspaces(db.clone(), &binding, &epoch, parameters).await?;
            let run = repository::create_manual_pipeline_run(
                db.as_ref(),
                binding.pipeline_id,
                parameters,
                Some(binding.pipeline_revision),
                None,
                Some(format!("orchestration:{}:{}", binding.id, command.epoch)),
                PipelineExecutionContext {
                    orchestration_binding_id: Some(binding.id),
                    execution_epoch: Some(command.epoch),
                    start_member: epoch.start_member.clone(),
                },
            )
            .await?;
            db.bind_orchestration_epoch_run(binding.id, command.epoch, run.id, chrono::Utc::now())
                .await?;
            let admission = db
                .fetch_ingress_admission(
                    binding.org_id,
                    binding.scope.clone(),
                    binding.correlation_key.clone(),
                )
                .await?;
            if admission
                .as_ref()
                .is_some_and(|admission| admission.pipeline_run_id.is_none())
            {
                let _ = db
                    .bind_ingress_pipeline_run(binding.admission_id, run.id, chrono::Utc::now())
                    .await?;
            }
            Ok(runinator_models::json!({ "pipeline_run_id": run.id }))
        }
        "cancel_epoch" => {
            if let Some(run_id) = epoch.pipeline_run_id {
                repository::cancel_pipeline_run(db.as_ref(), broker, run_id).await?;
            }
            abandon_canceled_epoch_workspaces(db.as_ref(), &binding, command.epoch).await?;
            Ok(runinator_models::json!({ "canceled": epoch.pipeline_run_id }))
        }
        "pause_epoch" => {
            if let Some(run_id) = epoch.pipeline_run_id {
                repository::pause_pipeline_run(db.as_ref(), run_id).await?;
            }
            Ok(runinator_models::json!({ "paused": epoch.pipeline_run_id }))
        }
        "resume_epoch" => {
            if let Some(run_id) = epoch.pipeline_run_id {
                repository::resume_pipeline_run(db.as_ref(), run_id).await?;
            }
            Ok(runinator_models::json!({ "resumed": epoch.pipeline_run_id }))
        }
        "signal_epoch" => {
            let Some(run_id) = epoch.pipeline_run_id else {
                return Ok(Value::Null);
            };
            let Some(member_key) = command.payload.get("member").and_then(Value::as_str) else {
                return Ok(runinator_models::json!({ "ignored": "missing member target" }));
            };
            let workflow_run_id = select_active_member_workflow_run(
                &db.fetch_pipeline_member_attempts(run_id).await?,
                member_key,
            );
            if let Some(workflow_run_id) = workflow_run_id {
                repository::request_run_interrupt(
                    db.as_ref(),
                    workflow_run_id,
                    InterruptSource::External,
                    command.payload.clone(),
                    None,
                )
                .await?;
                Ok(runinator_models::json!({ "workflow_run_id": workflow_run_id }))
            } else {
                Ok(runinator_models::json!({ "ignored": "no active member" }))
            }
        }
        "arm_intent_wake" => {
            let intent = command
                .payload
                .get("intent")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "orchestration wake command is missing its intent",
                    )) as runinator_models::errors::SendableError
                })?;
            let wake_at_ms = command
                .payload
                .get("wake_at_ms")
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "orchestration wake command is missing its deadline",
                    )) as runinator_models::errors::SendableError
                })?;
            let due_at = chrono::DateTime::from_timestamp_millis(wake_at_ms).ok_or_else(|| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "orchestration wake command has an invalid deadline",
                )) as runinator_models::errors::SendableError
            })?;
            let wake =
                WakeCommand::orchestration_intent(due_at, binding.id, intent, uuid::Uuid::now_v7());
            match broker
                .publish_wake(WakeMessage {
                    dedupe_key: Some(wake.dedupe_key()),
                    command: wake,
                    enqueued_at: chrono::Utc::now(),
                })
                .await
            {
                Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {
                    Ok(runinator_models::json!({ "armed": due_at, "intent": intent }))
                }
                Err(error) => Err(Box::new(error)),
            }
        }
        other => Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown orchestration command '{other}'"),
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrchestrationCommandFence {
    Execute,
    Retry,
    Stale(&'static str),
}

fn orchestration_command_fence(
    current_epoch: i64,
    status: OrchestrationStatus,
    command_epoch: i64,
    command_type: &str,
) -> OrchestrationCommandFence {
    match command_type {
        "start_epoch" if command_epoch > current_epoch => OrchestrationCommandFence::Retry,
        "start_epoch"
            if command_epoch < current_epoch || status != OrchestrationStatus::Running =>
        {
            OrchestrationCommandFence::Stale(
                "start command no longer targets the current running epoch",
            )
        }
        "pause_epoch"
            if command_epoch != current_epoch || status != OrchestrationStatus::Suspended =>
        {
            OrchestrationCommandFence::Stale(
                "pause command no longer targets the current suspended epoch",
            )
        }
        "resume_epoch" | "signal_epoch"
            if command_epoch != current_epoch || status != OrchestrationStatus::Running =>
        {
            OrchestrationCommandFence::Stale("command no longer targets the current running epoch")
        }
        _ => OrchestrationCommandFence::Execute,
    }
}

async fn record_stale_orchestration_command<T: OrchestrationStore>(
    db: &T,
    binding: &runinator_models::orchestration::OrchestrationBinding,
    command: &runinator_models::orchestration::OrchestrationCommand,
    reason: &str,
) -> Result<runinator_models::value::Value, runinator_models::errors::SendableError> {
    let detail = runinator_models::json!({
        "ignored": "stale_command",
        "reason": reason,
        "command_id": command.id,
        "command_type": command.command_type,
        "command_epoch": command.epoch,
        "current_epoch": binding.current_epoch,
        "binding_status": binding.status.as_str(),
        "operation_key": command.operation_key,
    });
    db.append_orchestration_evidence(OrchestrationEvidence {
        id: uuid::Uuid::now_v7(),
        binding_id: binding.id,
        epoch: Some(command.epoch),
        kind: "stale_orchestration_command".into(),
        subject_revision: binding.subject_revision.clone(),
        payload: detail.clone(),
        source_event_id: None,
        created_at: chrono::Utc::now(),
    })
    .await?;
    Ok(detail)
}

async fn prepare_epoch_workspaces<T: DefinitionStore + WorkspaceStore + ReplicaStore>(
    db: Arc<T>,
    binding: &runinator_models::orchestration::OrchestrationBinding,
    epoch: &runinator_models::orchestration::OrchestrationEpoch,
    mut parameters: Value,
) -> Result<Value, runinator_models::errors::SendableError> {
    let revision = db
        .fetch_pipeline_revision(binding.pipeline_id, binding.pipeline_revision)
        .await?
        .ok_or_else(|| {
            Box::new(std::io::Error::other(
                "orchestration pipeline revision disappeared",
            )) as runinator_models::errors::SendableError
        })?;
    let members = if let Some(start_member) = epoch.start_member.as_deref() {
        vec![start_member.to_string()]
    } else {
        let downstream = revision
            .graph
            .links
            .iter()
            .filter(|link| link.enabled)
            .map(|link| link.to.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        revision
            .graph
            .members
            .iter()
            .filter(|member| !downstream.contains(member.key.as_str()))
            .map(|member| member.key.clone())
            .collect()
    };
    let operations = WorkspaceOperations::new(db.clone());
    let mut workspaces = runinator_models::value::Map::new();
    for member in members {
        let Some(policy) = binding
            .policy
            .phases
            .get(&member)
            .and_then(|phase| phase.workspace.as_ref())
        else {
            continue;
        };
        let required_labels = workspace_labels(&policy.requirements)?;
        let mut attempt = if policy.reuse { 1 } else { epoch.epoch };
        let workspace = loop {
            let workspace = operations
                .allocate(crate::services::WorkspaceAllocationRequest {
                    admission_id: binding.admission_id,
                    generation: binding.generation,
                    scope: policy.scope.clone(),
                    attempt,
                    required_labels: required_labels.clone(),
                    lease_seconds: i64::try_from(policy.lease_seconds).ok(),
                })
                .await?;
            if !workspace.status.is_terminal() {
                break workspace;
            }
            match policy.recovery {
                runinator_models::orchestration::WorkspaceRecovery::Replace => {
                    attempt = attempt.saturating_add(1);
                }
                runinator_models::orchestration::WorkspaceRecovery::Wait => {
                    return Err(Box::new(std::io::Error::other(format!(
                        "workspace '{}' is waiting for recovery",
                        policy.scope
                    ))));
                }
                runinator_models::orchestration::WorkspaceRecovery::Fail => {
                    return Err(Box::new(std::io::Error::other(format!(
                        "workspace '{}' is unavailable",
                        policy.scope
                    ))));
                }
            }
        };
        let workspace =
            if workspace.status == runinator_models::workspaces::WorkspaceStatus::Allocating {
                operations
                    .activate(workspace.id, workspace.version)
                    .await?
                    .unwrap_or(workspace)
            } else {
                workspace
            };
        workspaces.insert(
            member,
            Value::from(serde_json::to_value(workspace.affinity())?),
        );
    }
    let parameters_object = parameters.as_object_mut().ok_or_else(|| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace-enabled epoch parameters must be an object",
        )) as runinator_models::errors::SendableError
    })?;
    let mut orchestration = match parameters_object.remove("orchestration") {
        Some(Value::Object(values)) => values,
        _ => runinator_models::value::Map::new(),
    };
    if workspaces.len() == 1 {
        orchestration.insert(
            "workspace_affinity".into(),
            workspaces.values().next().cloned().unwrap_or_default(),
        );
    }
    orchestration.insert("binding_id".into(), Value::String(binding.id.to_string()));
    orchestration.insert("generation".into(), Value::from(binding.generation));
    orchestration.insert("epoch".into(), Value::from(epoch.epoch));
    orchestration.insert("configuration".into(), binding.policy.defaults.clone());
    orchestration.insert(
        "configuration_version".into(),
        runinator_models::json!({
            "pipeline_revision": binding.pipeline_revision,
            "pipeline_digest": binding.pipeline_digest,
            "adapter_revision": binding.adapter_revision,
        }),
    );
    orchestration.insert(
        "current_attempt".into(),
        Value::from(binding.current_attempt),
    );
    orchestration.insert(
        "budget_counters".into(),
        Value::from(serde_json::to_value(&binding.budgets)?),
    );
    orchestration.insert(
        "budget_limits".into(),
        Value::from(serde_json::to_value(&binding.policy.budgets)?),
    );
    orchestration.insert(
        "subject_revision".into(),
        binding
            .subject_revision
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    orchestration.insert("resources".into(), binding.resources.clone());
    orchestration.insert("workspaces".into(), Value::Object(workspaces));
    parameters_object.insert("orchestration".into(), Value::Object(orchestration));
    Ok(parameters)
}

fn workspace_labels(
    requirements: &Value,
) -> Result<BTreeMap<String, String>, runinator_models::errors::SendableError> {
    let Some(values) = requirements.as_object() else {
        if requirements.is_null() {
            return Ok(BTreeMap::new());
        }
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace requirements must be an object of worker labels",
        )));
    };
    values
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_string()))
                .ok_or_else(|| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("workspace label '{key}' must be a string"),
                    )) as runinator_models::errors::SendableError
                })
        })
        .collect()
}

async fn settle_current_orchestration_epoch<
    T: RuntimeStore + IngressStore + OrchestrationStore + WorkspaceStore,
>(
    db: &T,
    owner: &str,
    binding: &runinator_models::orchestration::OrchestrationBinding,
) -> Result<(), runinator_models::errors::SendableError> {
    if binding.status.is_terminal() {
        return Ok(());
    }
    let Some(epoch) = db
        .fetch_orchestration_epochs(binding.id)
        .await?
        .into_iter()
        .find(|epoch| epoch.epoch == binding.current_epoch)
    else {
        return Ok(());
    };
    let Some(run_id) = epoch.pipeline_run_id else {
        return Ok(());
    };
    let Some(run) = db.fetch_pipeline_run(run_id).await? else {
        return Ok(());
    };
    let attempts = db.fetch_pipeline_member_attempts(run_id).await?;
    let phase_attempt = select_epoch_phase_attempt(&attempts, run.status);
    if !run.status.is_terminal() {
        if let Some(attempt) = phase_attempt
            && (binding.current_phase.as_deref() != Some(attempt.member_key.as_str())
                || binding.current_attempt != attempt.attempt)
        {
            let now = chrono::Utc::now();
            let update = OrchestrationBindingUpdate {
                expected_version: binding.version,
                status: binding.status,
                current_phase: Some(attempt.member_key.clone()),
                current_attempt: attempt.attempt,
                current_epoch: binding.current_epoch,
                restart_member: binding.restart_member.clone(),
                resume_existing_epoch: binding.resume_existing_epoch,
                subject_revision: binding.subject_revision.clone(),
                resources: binding.resources.clone(),
                budgets: binding.budgets.clone(),
                last_reduced_sequence: binding.last_reduced_sequence,
                finished_at: binding.finished_at,
            };
            let _ = db
                .update_orchestration_binding(binding.id, owner.to_string(), update, now)
                .await?;
        }
        return Ok(());
    }
    if binding.status == OrchestrationStatus::Suspended {
        return Ok(());
    }

    let now = chrono::Utc::now();
    db.settle_orchestration_epoch(
        binding.id,
        binding.current_epoch,
        run.status.as_str().to_string(),
        now,
    )
    .await?;
    let mut status = OrchestrationStatus::Completed;
    let mut current_phase = binding.current_phase.clone();
    let mut current_attempt = binding.current_attempt;
    let mut current_epoch = binding.current_epoch;
    let mut restart_member = binding.restart_member.clone();
    let mut subject_revision = binding.subject_revision.clone();
    let mut resources = binding.resources.clone();
    let mut budgets = binding.budgets.clone();
    let mut mapped_evidence = None;
    let mut mapped_correlations = Vec::new();
    let mut next_epoch = None;
    let handoff_outcome = epoch
        .reason
        .strip_prefix("budget_exhaustion:")
        .and_then(parse_budget_exhaustion);

    if let Some(attempt) = phase_attempt {
        current_phase = Some(attempt.member_key.clone());
        current_attempt = attempt.attempt;
        if let Some(phase) = binding.policy.phases.get(&attempt.member_key) {
            if let Some(pointer) = phase.result.subject_revision.as_deref()
                && let Some(revision) = attempt.result.pointer(pointer).and_then(Value::as_str)
            {
                subject_revision = Some(revision.to_string());
            }
            if let Some(pointer) = phase.result.resources.as_deref()
                && let Some(mapped) = attempt.result.pointer(pointer)
            {
                resources = mapped.clone();
            }
            if let Some(pointer) = phase.result.evidence.as_deref()
                && let Some(mapped) = attempt.result.pointer(pointer)
            {
                mapped_evidence = Some(mapped.clone());
            }
            if let Some(pointer) = phase.result.correlations.as_deref()
                && let Some(items) = attempt.result.pointer(pointer).and_then(Value::as_array)
            {
                for item in items {
                    let Some(item) = item.as_object() else {
                        continue;
                    };
                    let Some(source) = item.get("source").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(scope) = item.get("scope").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(correlation_key) = item.get("correlation_key").and_then(Value::as_str)
                    else {
                        continue;
                    };
                    if validate_correlation_alias_identity(source, scope, correlation_key).is_err()
                    {
                        continue;
                    }
                    mapped_correlations.push((
                        source.to_string(),
                        scope.to_string(),
                        correlation_key.to_string(),
                    ));
                }
            }
        }
    }

    if handoff_outcome.is_none()
        && run.status != runinator_models::workflows::WorkflowStatus::Succeeded
    {
        status = OrchestrationStatus::Failed;
        if let Some(attempt) = phase_attempt
            && let Some(phase) = binding.policy.phases.get(&attempt.member_key)
            && let Some(pointer) = phase.result.failure_class.as_deref()
            && let Some(failure_class) = attempt.result.pointer(pointer).and_then(Value::as_str)
            && let Some(decision) =
                consume_failure_budget(&binding.policy.budgets, &mut budgets, failure_class)
        {
            match decision {
                FailureBudgetDecision::Retry => {
                    current_epoch += 1;
                    status = OrchestrationStatus::Running;
                    restart_member = Some(attempt.member_key.clone());
                    next_epoch = Some((
                        current_epoch,
                        attempt.member_key.clone(),
                        epoch.parameters.clone(),
                        format!("failure budget '{failure_class}' retry"),
                    ));
                }
                FailureBudgetDecision::Exhausted { outcome, handoff } => {
                    if let Some(handoff) = handoff {
                        current_epoch += 1;
                        status = OrchestrationStatus::Running;
                        restart_member = Some(handoff.clone());
                        let mut parameters = epoch.parameters.clone();
                        if let Some(object) = parameters.as_object_mut() {
                            let orchestration = object
                                .entry("orchestration")
                                .or_insert_with(|| Value::Object(Default::default()));
                            if let Some(orchestration) = orchestration.as_object_mut() {
                                orchestration.insert(
                                    "exhaustion".into(),
                                    runinator_models::json!({
                                        "budget": failure_class,
                                        "used": budgets.get(failure_class).copied().unwrap_or_default(),
                                        "outcome": budget_exhaustion_name(outcome),
                                        "failed_phase": attempt.member_key,
                                    }),
                                );
                            }
                        }
                        next_epoch = Some((
                            current_epoch,
                            handoff,
                            parameters,
                            format!("budget_exhaustion:{}", budget_exhaustion_name(outcome)),
                        ));
                    } else {
                        status = status_for_budget_exhaustion(outcome);
                        if status == OrchestrationStatus::Suspended {
                            restart_member = Some(attempt.member_key.clone());
                        }
                    }
                }
            }
        }
    }

    if let Some(outcome) = handoff_outcome {
        status = status_for_budget_exhaustion(outcome);
    }

    if let Some((next_epoch, member, parameters, reason)) = &next_epoch {
        db.create_orchestration_epoch(
            NewOrchestrationEpoch {
                id: uuid::Uuid::now_v7(),
                binding_id: binding.id,
                epoch: *next_epoch,
                start_member: Some(member.clone()),
                parameters: parameters.clone(),
                reason: reason.clone(),
            },
            now,
        )
        .await?;
        db.enqueue_orchestration_command(
            NewOrchestrationCommand {
                id: uuid::Uuid::now_v7(),
                binding_id: binding.id,
                epoch: *next_epoch,
                command_type: "start_epoch".into(),
                operation_key: format!("epoch:{next_epoch}:start"),
                payload: runinator_models::json!({
                    "parameters": parameters,
                    "start_member": member,
                    "reason": reason,
                }),
            },
            now,
        )
        .await?;
    }

    let finished_at = status.is_terminal().then_some(now);
    let update = OrchestrationBindingUpdate {
        expected_version: binding.version,
        status,
        current_phase,
        current_attempt,
        current_epoch,
        restart_member,
        resume_existing_epoch: false,
        subject_revision: subject_revision.clone(),
        resources,
        budgets,
        last_reduced_sequence: binding.last_reduced_sequence,
        finished_at,
    };
    if let Some(updated) = db
        .update_orchestration_binding(binding.id, owner.to_string(), update, now)
        .await?
    {
        for (source, scope, correlation_key) in mapped_correlations {
            db.upsert_orchestration_correlation_alias(
                NewOrchestrationCorrelationAlias {
                    id: uuid::Uuid::now_v7(),
                    binding_id: binding.id,
                    generation: binding.generation,
                    org_id: binding.org_id,
                    source,
                    scope,
                    correlation_key,
                },
                now,
            )
            .await?;
        }
        settle_epoch_workspaces(db, &updated, binding.current_epoch).await?;
        if let Some(payload) = mapped_evidence {
            db.append_orchestration_evidence(OrchestrationEvidence {
                id: uuid::Uuid::now_v7(),
                binding_id: binding.id,
                epoch: Some(binding.current_epoch),
                kind: updated
                    .current_phase
                    .as_deref()
                    .map(|phase| format!("phase_result:{phase}"))
                    .unwrap_or_else(|| "phase_result".into()),
                subject_revision,
                payload,
                source_event_id: None,
                created_at: now,
            })
            .await?;
        }
        if updated.status.is_terminal() {
            db.settle_ingress_admission(binding.admission_id, binding.generation, now)
                .await?;
        }
    }
    Ok(())
}

async fn abandon_canceled_epoch_workspaces<T: WorkspaceStore>(
    db: &T,
    binding: &runinator_models::orchestration::OrchestrationBinding,
    canceled_epoch: i64,
) -> Result<(), runinator_models::errors::SendableError> {
    let workspaces = db
        .fetch_workspaces_for_admission(binding.admission_id, binding.generation)
        .await?;
    for workspace in workspaces
        .into_iter()
        .filter(|workspace| !workspace.status.is_terminal())
    {
        let reusable = binding.policy.phases.values().any(|phase| {
            phase
                .workspace
                .as_ref()
                .is_some_and(|policy| policy.scope == workspace.scope && policy.reuse)
        });
        if !should_abandon_canceled_workspace(
            binding.status,
            reusable,
            workspace.attempt,
            canceled_epoch,
        ) {
            continue;
        }
        let _ = db
            .transition_workspace_cas(
                workspace.id,
                workspace.version,
                workspace.status,
                runinator_models::workspaces::WorkspaceStatus::Finalizing,
                Some(runinator_models::json!({
                    "reason": "execution epoch canceled; cleanup required",
                    "epoch": canceled_epoch,
                    "binding_status": binding.status.as_str(),
                })),
                chrono::Utc::now(),
            )
            .await?;
    }
    Ok(())
}

fn should_abandon_canceled_workspace(
    binding_status: OrchestrationStatus,
    reusable: bool,
    workspace_attempt: i64,
    canceled_epoch: i64,
) -> bool {
    binding_status.is_terminal() || (!reusable && workspace_attempt == canceled_epoch)
}

async fn settle_epoch_workspaces<T: WorkspaceStore>(
    db: &T,
    binding: &runinator_models::orchestration::OrchestrationBinding,
    settled_epoch: i64,
) -> Result<(), runinator_models::errors::SendableError> {
    let workspaces = db
        .fetch_workspaces_for_admission(binding.admission_id, binding.generation)
        .await?;
    for workspace in workspaces
        .into_iter()
        .filter(|workspace| !workspace.status.is_terminal())
    {
        let reusable = binding.policy.phases.values().any(|phase| {
            phase
                .workspace
                .as_ref()
                .is_some_and(|policy| policy.scope == workspace.scope && policy.reuse)
        });
        if !binding.status.is_terminal() && reusable {
            continue;
        }
        let now = chrono::Utc::now();
        let evidence = runinator_models::json!({
            "reason": if binding.status.is_terminal() { "binding settled" } else { "epoch replaced" },
            "epoch": settled_epoch,
            "binding_status": binding.status.as_str(),
        });
        if binding.status.is_terminal() {
            if db
                .transition_workspace_cas(
                    workspace.id,
                    workspace.version,
                    workspace.status,
                    runinator_models::workspaces::WorkspaceStatus::Finalizing,
                    None,
                    now,
                )
                .await?
            {}
        } else {
            let _ = db
                .transition_workspace_cas(
                    workspace.id,
                    workspace.version,
                    workspace.status,
                    runinator_models::workspaces::WorkspaceStatus::Finalizing,
                    Some(evidence),
                    now,
                )
                .await?;
        }
    }
    Ok(())
}

fn select_epoch_phase_attempt(
    attempts: &[PipelineMemberAttempt],
    run_status: runinator_models::workflows::WorkflowStatus,
) -> Option<&PipelineMemberAttempt> {
    let failed_epoch = run_status.is_terminal()
        && run_status != runinator_models::workflows::WorkflowStatus::Succeeded;
    let failures = || {
        attempts.iter().filter(|attempt| {
            matches!(
                attempt.status,
                PipelineMemberAttemptStatus::Failed | PipelineMemberAttemptStatus::TimedOut
            )
        })
    };
    let by_recency = |attempt: &&PipelineMemberAttempt| {
        (
            attempt
                .finished_at
                .or(attempt.started_at)
                .unwrap_or(attempt.created_at),
            attempt.attempt,
        )
    };
    if failed_epoch && let Some(attempt) = failures().max_by_key(by_recency) {
        return Some(attempt);
    }
    attempts
        .iter()
        .filter(|attempt| attempt.status != PipelineMemberAttemptStatus::Skipped)
        .max_by_key(by_recency)
}

fn select_active_member_workflow_run(
    attempts: &[PipelineMemberAttempt],
    member_key: &str,
) -> Option<uuid::Uuid> {
    attempts
        .iter()
        .filter(|attempt| {
            attempt.member_key == member_key
                && !attempt.status.is_terminal()
                && attempt.workflow_run_id.is_some()
        })
        .max_by_key(|attempt| attempt.attempt)
        .and_then(|attempt| attempt.workflow_run_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FailureBudgetDecision {
    Retry,
    Exhausted {
        outcome: BudgetExhaustion,
        handoff: Option<String>,
    },
}

fn consume_failure_budget(
    policies: &BTreeMap<String, runinator_models::orchestration::BudgetPolicy>,
    counters: &mut BTreeMap<String, u32>,
    failure_class: &str,
) -> Option<FailureBudgetDecision> {
    let policy = policies.get(failure_class)?;
    let used = counters.entry(failure_class.to_string()).or_default();
    *used = used.saturating_add(1);
    if *used < policy.attempts {
        return Some(FailureBudgetDecision::Retry);
    }
    Some(FailureBudgetDecision::Exhausted {
        outcome: policy.exhausted,
        handoff: policy.handoff.clone(),
    })
}

fn budget_exhaustion_name(outcome: BudgetExhaustion) -> &'static str {
    match outcome {
        BudgetExhaustion::Fail => "fail",
        BudgetExhaustion::Pause => "pause",
        BudgetExhaustion::Terminate => "terminate",
    }
}

fn parse_budget_exhaustion(value: &str) -> Option<BudgetExhaustion> {
    match value {
        "fail" => Some(BudgetExhaustion::Fail),
        "pause" => Some(BudgetExhaustion::Pause),
        "terminate" => Some(BudgetExhaustion::Terminate),
        _ => None,
    }
}

fn status_for_budget_exhaustion(outcome: BudgetExhaustion) -> OrchestrationStatus {
    match outcome {
        BudgetExhaustion::Fail => OrchestrationStatus::Failed,
        BudgetExhaustion::Pause => OrchestrationStatus::Suspended,
        BudgetExhaustion::Terminate => OrchestrationStatus::Terminated,
    }
}
use tokio::sync::Notify;
use tracing::{error, info, warn};

use crate::{
    events::{
        AppEventKind, EventSender, emit, emit_pipeline_run, emit_workflow_run,
        emit_workflow_run_resolved,
    },
    repository,
    services::{ReplicaRegistry, WorkspaceOperations, WorkspaceRecovery},
    settings::ServerSettingsHandle,
    stability,
};

fn queue_age(
    now: chrono::DateTime<chrono::Utc>,
    oldest: Option<chrono::DateTime<chrono::Utc>>,
) -> u64 {
    oldest
        .map(|value| (now - value).num_seconds().max(0) as u64)
        .unwrap_or(0)
}

/// Drive compiled workflow continuations. Effect publication is intentionally separate: the VM
/// host writes an effect outbox record which the generic dispatcher drains.
pub async fn run_workflow_vm_driver<
    T: RuntimeStore + WorkflowVmStore + IngressStore + DefinitionStore,
>(
    db: Arc<T>,
    instance: String,
    ready_nudge: Arc<Notify>,
    events: EventSender,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM driver started");
    let host = runinator_runtime::WorkflowVmHost::new(db.as_ref());
    loop {
        let started = std::time::Instant::now();
        let mut succeeded = true;
        let policy = settings.current();
        let claim_limit = policy.orchestration.claim_batch_size as i64;
        match host.drive_runnable(instance.clone(), claim_limit).await {
            Ok(outcomes) => {
                for outcome in outcomes {
                    let workflow_run_id = outcome.workflow_run_id();
                    stability::vm_continuation_driven(match outcome {
                        runinator_runtime::WorkflowVmDriveOutcome::Yielded { .. } => "yielded",
                        runinator_runtime::WorkflowVmDriveOutcome::Forked { .. } => "forked",
                        runinator_runtime::WorkflowVmDriveOutcome::Joined { .. } => "joined",
                        runinator_runtime::WorkflowVmDriveOutcome::Completed { .. } => "completed",
                        runinator_runtime::WorkflowVmDriveOutcome::Failed { .. } => "failed",
                        runinator_runtime::WorkflowVmDriveOutcome::Interrupted { .. } => {
                            "interrupted"
                        }
                        runinator_runtime::WorkflowVmDriveOutcome::InterruptResolved { .. } => {
                            "interrupt_resolved"
                        }
                    });
                    let settled_run_id = match outcome {
                        runinator_runtime::WorkflowVmDriveOutcome::Completed {
                            settled_run_id,
                            ..
                        }
                        | runinator_runtime::WorkflowVmDriveOutcome::Failed {
                            settled_run_id,
                            ..
                        }
                        | runinator_runtime::WorkflowVmDriveOutcome::InterruptResolved {
                            settled_run_id,
                            ..
                        } => settled_run_id,
                        _ => None,
                    };
                    if let Some(run_id) = settled_run_id {
                        match db
                            .settle_and_promote_ingress_workflow_run(run_id, chrono::Utc::now())
                            .await
                        {
                            Ok(Some(promotion)) => {
                                start_ingress_promotion(db.as_ref(), promotion).await
                            }
                            Ok(None) => {}
                            Err(err) => {
                                warn!(workflow_run_id = %run_id, error = %err, "ingress workflow settlement failed")
                            }
                        }
                        if let Err(err) =
                            repository::advance_pipeline_from_vm_terminal(db.as_ref(), run_id).await
                        {
                            warn!(workflow_run_id = %run_id, error = %err, "VM pipeline advancement failed");
                        }
                        match db.fetch_workflow_run(run_id).await {
                            Ok(Some(run)) => {
                                if let Some(pipeline_run_id) = run.pipeline_run_id {
                                    match db.fetch_pipeline_run(pipeline_run_id).await {
                                        Ok(Some(pipeline_run))
                                            if pipeline_run.status.is_terminal()
                                                && pipeline_run
                                                    .orchestration_binding_id
                                                    .is_none() =>
                                        {
                                            match db
                                                .settle_and_promote_ingress_pipeline_run(
                                                    pipeline_run_id,
                                                    chrono::Utc::now(),
                                                )
                                                .await
                                            {
                                                Ok(Some(promotion)) => {
                                                    start_ingress_promotion(db.as_ref(), promotion)
                                                        .await
                                                }
                                                Ok(None) => {}
                                                Err(err) => {
                                                    warn!(pipeline_run_id = %pipeline_run_id, error = %err, "ingress pipeline settlement failed")
                                                }
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(err) => {
                                            warn!(pipeline_run_id = %pipeline_run_id, error = %err, "failed to load pipeline run for ingress settlement")
                                        }
                                    }
                                }
                                if let Err(err) =
                                    repository::maybe_start_chained_pipelines(db.as_ref(), &run)
                                        .await
                                {
                                    warn!(workflow_run_id = %run_id, error = %err, "VM chained pipeline advancement failed");
                                }
                            }
                            Ok(None) => {}
                            Err(err) => {
                                warn!(workflow_run_id = %run_id, error = %err, "failed to load terminal VM run for pipeline chaining")
                            }
                        }
                    }
                    emit_workflow_run_resolved(db.as_ref(), &events, workflow_run_id).await;
                }
            }
            Err(err) => {
                succeeded = false;
                stability::vm_driver_failure();
                warn!(error = %err, "workflow VM drive failed");
            }
        }
        match db.fetch_unsettled_vm_pipeline_members(claim_limit).await {
            Ok(run_ids) => {
                for run_id in run_ids {
                    if let Err(err) =
                        repository::advance_pipeline_from_vm_terminal(db.as_ref(), run_id).await
                    {
                        warn!(workflow_run_id = %run_id, error = %err, "VM pipeline reconciliation failed");
                    }
                }
            }
            Err(err) => {
                succeeded = false;
                warn!(error = %err, "failed to reconcile VM pipeline members");
            }
        }
        // A startup failure releases its claim back to the FIFO head. Reconciliation retries one
        // such head each driver pass, including after process restart.
        match db.claim_queued_ingress_event(chrono::Utc::now()).await {
            Ok(Some(promotion)) => start_ingress_promotion(db.as_ref(), promotion).await,
            Ok(None) => {}
            Err(err) => warn!(error = %err, "failed to reconcile queued ingress event"),
        }
        stability::record_vm_drive_duration_ms(started.elapsed().as_secs_f64() * 1000.0);
        stability::loop_iteration("workflow_vm_driver", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = ready_nudge.notified() => {}
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.workflow_vm_poll_interval_ms)) => {}
        }
    }
}

async fn start_ingress_promotion<
    T: RuntimeStore + WorkflowVmStore + IngressStore + DefinitionStore,
>(
    db: &T,
    promotion: IngressPromotion,
) {
    let event_id = promotion.event.id;
    let admission_id = promotion.admission.id.expect("stored admission id");
    let result = match promotion.admission.target.kind {
        IngressTargetKind::Workflow => {
            let provenance = WorkflowRunProvenance {
                source_kind: Some(TriggerSourceKind::Api),
                actor_type: Some(TriggerActorType::System),
                actor_replica_id: None,
                actor_display_name: Some("ingress queue".into()),
                request_host: None,
                request_ip: None,
                metadata: runinator_models::json!({
                    "ingress_source": promotion.event.source,
                    "ingress_event_id": promotion.event.event_id,
                    "ingress_generation": promotion.admission.generation,
                }),
            };
            match repository::create_workflow_run(
                db,
                promotion.admission.target.id,
                promotion.event.payload.clone(),
                false,
                Some(format!("ingress:{}", promotion.event.event_id)),
                provenance,
            )
            .await
            {
                Ok(run) => db
                    .bind_ingress_workflow_run(admission_id, run.id, chrono::Utc::now())
                    .await
                    .and_then(|bound| {
                        if bound {
                            Ok(Some((Some(run.id), None)))
                        } else {
                            Err(Box::new(std::io::Error::other(
                                "promoted workflow admission bind lost",
                            )))
                        }
                    }),
                Err(err) => Err(err),
            }
        }
        IngressTargetKind::Pipeline => match repository::create_manual_pipeline_run(
            db,
            promotion.admission.target.id,
            promotion.event.payload.clone(),
            None,
            None,
            Some("ingress queue".into()),
            Default::default(),
        )
        .await
        {
            Ok(run) => db
                .bind_ingress_pipeline_run(admission_id, run.id, chrono::Utc::now())
                .await
                .and_then(|bound| {
                    if bound {
                        Ok(Some((None, Some(run.id))))
                    } else {
                        Err(Box::new(std::io::Error::other(
                            "promoted pipeline admission bind lost",
                        )))
                    }
                }),
            Err(err) => Err(err),
        },
    };
    match result {
        Ok(Some((workflow_run_id, pipeline_run_id))) => {
            if let Err(err) = db
                .bind_ingress_event_result(
                    event_id,
                    workflow_run_id,
                    pipeline_run_id,
                    chrono::Utc::now(),
                )
                .await
            {
                warn!(ingress_event_id = %event_id, error = %err, "failed to bind promoted ingress event result");
            }
        }
        Ok(None) => {}
        Err(err) => {
            warn!(ingress_event_id = %event_id, error = %err, "queued ingress startup failed; releasing FIFO claim");
            if let Err(release_err) = db
                .release_ingress_promotion(promotion.claim_token, chrono::Utc::now())
                .await
            {
                warn!(ingress_event_id = %event_id, error = %release_err, "failed to release queued ingress claim");
            }
        }
    }
}

/// Arm declared periodic interrupt timers through the broker-only waker.
///
/// The schedule itself is durable; re-publishing the same not-yet-due occurrence is harmless
/// because the wake key includes the run, timer declaration, and exact due instant. This keeps the
/// engine from sleeping on a run-local timer and lets any waker relay the due occurrence.
pub async fn run_timer_interrupt_scheduler<T: WorkflowVmStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("workflow timer-interrupt scheduler started");
    loop {
        let started = std::time::Instant::now();
        let now = chrono::Utc::now();
        let policy = settings.current();
        let mut succeeded = true;
        match db
            .fetch_workflow_timer_interrupts_before(
                now + chrono::Duration::milliseconds(
                    policy.orchestration.timer_arm_horizon_ms as i64,
                ),
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(timers) => {
                for timer in timers {
                    let wake = WakeCommand::timer_interrupt(
                        timer.due_at,
                        timer.workflow_run_id,
                        timer.timer_id.clone(),
                        timer.interval_seconds,
                        uuid::Uuid::now_v7(),
                    );
                    match broker
                        .publish_wake(WakeMessage {
                            dedupe_key: Some(wake.dedupe_key()),
                            command: wake,
                            enqueued_at: now,
                        })
                        .await
                    {
                        Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {}
                        Err(err) => {
                            succeeded = false;
                            warn!(
                                workflow_run_id = %timer.workflow_run_id,
                                timer_id = %timer.timer_id,
                                error = %err,
                                "failed to arm workflow timer interrupt"
                            );
                        }
                    }
                }
            }
            Err(err) => {
                succeeded = false;
                warn!(error = %err, "failed to load workflow timer interrupts to arm");
            }
        }
        stability::loop_iteration("timer_interrupt_scheduler", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.workflow_vm_poll_interval_ms)) => {}
        }
    }
}

/// Drain the VM effect outbox. The command was frozen in the same transaction as the suspended
/// continuation, so this publisher never re-reads graph or node-run state to rebuild a delivery.
pub async fn run_workflow_effect_dispatcher<
    T: WorkflowVmStore + WorkspaceStore + OrchestrationStore + DefinitionStore,
>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    publisher: crate::events::EventSender,
    instance: String,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM effect dispatcher started");
    loop {
        let now = chrono::Utc::now();
        let policy = settings.current();
        match db
            .claim_pending_workflow_effect_dispatches(
                instance.clone(),
                now,
                now + chrono::Duration::seconds(
                    policy.orchestration.action_dispatch_lease_seconds as i64,
                ),
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(dispatches) => {
                for dispatch in dispatches {
                    match workspace_affinity_is_current(db.as_ref(), &dispatch.command.request)
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            let message = "workspace affinity is stale or no longer active";
                            if let Err(error) = db
                                .settle_workflow_effect(
                                    dispatch.command.effect_id,
                                    dispatch.command.attempt,
                                    WorkflowEffectStatus::Rejected,
                                    None,
                                    Some(message.into()),
                                    now,
                                )
                                .await
                            {
                                warn!(error = %error, dispatch_id = %dispatch.id, "failed to reject stale workspace effect");
                                let _ = db
                                    .mark_workflow_effect_dispatch_failed(
                                        dispatch.id,
                                        error.to_string(),
                                    )
                                    .await;
                                continue;
                            }
                            if let Err(error) = db
                                .mark_workflow_effect_dispatch_published(dispatch.id)
                                .await
                            {
                                warn!(error = %error, dispatch_id = %dispatch.id, "failed to acknowledge rejected workspace effect");
                            }
                            continue;
                        }
                        Err(error) => {
                            warn!(error = %error, dispatch_id = %dispatch.id, "failed to validate workspace affinity");
                            let _ = db
                                .mark_workflow_effect_dispatch_failed(
                                    dispatch.id,
                                    error.to_string(),
                                )
                                .await;
                            continue;
                        }
                    }
                    let operation = match prepare_external_operation(
                        db.as_ref(),
                        &dispatch.command,
                        now,
                    )
                    .await
                    {
                        Ok(operation) => operation,
                        Err(error) => {
                            warn!(error = %error, dispatch_id = %dispatch.id, "failed to prepare binding-scoped external operation");
                            let _ = db
                                .mark_workflow_effect_dispatch_failed(
                                    dispatch.id,
                                    error.to_string(),
                                )
                                .await;
                            continue;
                        }
                    };
                    // kept for the deadline arming below, since publishing consumes the command.
                    let published_command = dispatch.command.clone();
                    match broker
                        .publish_effect(runinator_broker_core::EffectMessage {
                            dedupe_key: Some(dispatch.dedupe_key.clone()),
                            command: dispatch.command,
                            enqueued_at: now,
                        })
                        .await
                    {
                        Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {
                            if let Some(operation) = operation {
                                let operation_id = operation.id;
                                let binding_id = operation.binding_id;
                                if let Err(error) = db
                                    .update_external_operation(
                                        operation.id,
                                        ExternalOperationUpdate {
                                            status: ExternalOperationStatus::Running,
                                            attempt: i64::from(published_command.attempt),
                                            ambiguous: false,
                                            provenance: operation.provenance,
                                            receipt: operation.receipt,
                                        },
                                        now,
                                    )
                                    .await
                                {
                                    warn!(error = %error, %operation_id, "failed to mark external operation running");
                                } else if let Ok(Some(binding)) =
                                    db.fetch_orchestration_binding(binding_id).await
                                {
                                    crate::events::emit_external_operation(
                                        &publisher,
                                        operation_id,
                                        binding_id,
                                        binding.org_id,
                                    );
                                }
                            }
                            // armed after publication, never before: the backstop must not be able
                            // to stop the work it protects.
                            crate::effect_deadline::arm_with_grace(
                                broker.as_ref(),
                                &published_command,
                                now,
                                policy.orchestration.action_deadline_grace_seconds as i64,
                            )
                            .await;
                            if let Err(err) = db
                                .mark_workflow_effect_dispatch_published(dispatch.id)
                                .await
                            {
                                warn!(error = %err, dispatch_id = %dispatch.id, "failed to acknowledge VM effect publication");
                            }
                        }
                        Err(err) => {
                            warn!(error = %err, dispatch_id = %dispatch.id, "failed to publish VM effect");
                            let _ = db
                                .mark_workflow_effect_dispatch_failed(dispatch.id, err.to_string())
                                .await;
                        }
                    }
                }
            }
            Err(err) => warn!(error = %err, "failed to claim VM effect dispatches"),
        }
        tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.effect_dispatch_poll_interval_ms)) => {} }
    }
}

async fn prepare_external_operation<T: OrchestrationStore + DefinitionStore>(
    db: &T,
    command: &runinator_comm::EffectCommand,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<ExternalOperation>, runinator_models::errors::SendableError> {
    let WorkflowEffectRequest::Action {
        provider,
        function,
        idempotency_key,
        ..
    } = &command.request
    else {
        return Ok(None);
    };
    let Some(binding) = db
        .fetch_current_orchestration_binding_for_workflow_run(command.workflow_run_id)
        .await?
    else {
        return Ok(None);
    };
    let semantics = match db
        .fetch_catalog_item(crate::repository::provider_catalog_uri(provider))
        .await?
    {
        Some(item) => crate::repository::provider_metadata_from_item(item)
            .ok()
            .and_then(|metadata| {
                metadata
                    .actions
                    .into_iter()
                    .find(|action| action.function_name == *function)
                    .map(|action| action.delivery_semantics)
            })
            .unwrap_or(DeliverySemantics::AtLeastOnce),
        None => DeliverySemantics::AtLeastOnce,
    };
    let provider_idempotency_key = idempotency_key.as_ref().map(operation_value_key);
    let operation = ExternalOperation {
        id: uuid::Uuid::now_v7(),
        binding_id: binding.id,
        epoch: binding.current_epoch,
        workflow_run_id: Some(command.workflow_run_id),
        effect_id: Some(command.effect_id),
        operation_key: command.idempotency_key.clone(),
        provider: provider.clone(),
        action: function.clone(),
        semantics,
        attempt: i64::from(command.attempt),
        status: ExternalOperationStatus::Pending,
        ambiguous: false,
        provenance: runinator_models::json!({
            "binding_id": binding.id,
            "generation": binding.generation,
            "epoch": binding.current_epoch,
            "workflow_run_id": command.workflow_run_id,
            "effect_id": command.effect_id,
            "provider_idempotency_key": provider_idempotency_key,
        }),
        receipt: runinator_models::value::Value::Null,
        created_at: now,
        updated_at: now,
    };
    db.create_external_operation(operation).await.map(Some)
}

fn operation_value_key(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        value => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}

async fn workspace_affinity_is_current<T: WorkspaceStore>(
    db: &T,
    request: &WorkflowEffectRequest,
) -> Result<bool, runinator_models::errors::SendableError> {
    let WorkflowEffectRequest::Action {
        workspace_affinity: Some(value),
        ..
    } = request
    else {
        return Ok(true);
    };
    let affinity: WorkspaceAffinity =
        serde_json::from_value(value.clone().into()).map_err(|error| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid workspace affinity: {error}"),
            )) as runinator_models::errors::SendableError
        })?;
    let Some(workspace) = db.fetch_workspace(affinity.workspace_id).await? else {
        return Ok(false);
    };
    Ok(workspace_affinity_matches(&workspace, &affinity))
}

fn workspace_affinity_matches(
    workspace: &runinator_models::workspaces::WorkspaceLease,
    affinity: &WorkspaceAffinity,
) -> bool {
    !workspace.status.is_terminal()
        && workspace.worker_instance_id == affinity.worker_instance_id
        && workspace.local_key == affinity.local_key
        && workspace.attempt == affinity.attempt
        && workspace.version == affinity.version
}

/// Drain the notification-owned provider-effect outbox. Notification records deliberately share
/// worker provider execution with VM effects while retaining their own persistence receipt and
/// settlement path.
pub async fn run_notification_effect_dispatcher<T: NotificationStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance: String,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("notification effect dispatcher started");
    loop {
        let now = chrono::Utc::now();
        let policy = settings.current();
        match db
            .claim_pending_notification_effect_dispatches(
                instance.clone(),
                now,
                now + chrono::Duration::seconds(
                    policy.orchestration.action_dispatch_lease_seconds as i64,
                ),
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(dispatches) => {
                for dispatch in dispatches {
                    match broker
                        .publish_effect(runinator_broker_core::EffectMessage {
                            dedupe_key: Some(dispatch.dedupe_key.clone()),
                            command: dispatch.command,
                            enqueued_at: now,
                        })
                        .await
                    {
                        Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {
                            if let Err(err) = db
                                .mark_notification_effect_dispatch_published(dispatch.delivery_id)
                                .await
                            {
                                warn!(error = %err, delivery_id = %dispatch.delivery_id, "failed to acknowledge notification effect publication");
                            }
                        }
                        Err(err) => {
                            warn!(error = %err, delivery_id = %dispatch.delivery_id, "failed to publish notification effect");
                            let _ = db
                                .mark_notification_effect_dispatch_failed(
                                    dispatch.delivery_id,
                                    err.to_string(),
                                )
                                .await;
                        }
                    }
                }
            }
            Err(err) => warn!(error = %err, "failed to claim notification effect dispatches"),
        }
        tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.effect_dispatch_poll_interval_ms)) => {} }
    }
}

/// Periodically samples durable operational state so an idle deployment still has useful gauges.
/// This deliberately queries only aggregate queue/fleet state and never emits record identities.
pub async fn run_operational_metrics_sampler<
    T: RuntimeStore + WorkflowVmStore + NotificationStore + OrgStore + ReplicaStore,
>(
    db: Arc<T>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("operational metrics sampler started");
    loop {
        let started = std::time::Instant::now();
        let now = chrono::Utc::now();
        let policy = settings.current();
        let mut succeeded = true;

        match db
            .agent_directive_queue_snapshot(now, now - chrono::Duration::seconds(30))
            .await
        {
            Ok(snapshot) => stability::queue_snapshot(
                "agent_directive",
                snapshot.depth,
                snapshot.claimed,
                queue_age(now, snapshot.oldest_enqueued_at),
            ),
            Err(err) => {
                succeeded = false;
                stability::queue_failure("agent_directive", "snapshot");
                warn!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "agent directive metrics snapshot failed: {err}"
                );
            }
        }
        match db.notification_delivery_queue_snapshot().await {
            Ok(snapshot) => stability::queue_snapshot(
                "notification_delivery",
                snapshot.depth,
                snapshot.claimed,
                queue_age(now, snapshot.oldest_enqueued_at),
            ),
            Err(err) => {
                succeeded = false;
                stability::queue_failure("notification_delivery", "snapshot");
                warn!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "notification delivery metrics snapshot failed: {err}"
                );
            }
        }

        let stale_before =
            now - chrono::Duration::seconds(policy.replicas.stale_after_seconds as i64);
        match db.fetch_replicas(None, None, stale_before).await {
            Ok(replicas) => {
                for kind in ReplicaKind::ALL {
                    for status in [
                        ReplicaStatus::Live,
                        ReplicaStatus::Stale,
                        ReplicaStatus::Offline,
                    ] {
                        let count = replicas
                            .iter()
                            .filter(|replica| {
                                replica.replica_type == *kind && replica.status == status
                            })
                            .count() as u64;
                        stability::replica_snapshot(kind.as_str(), status.as_str(), count);
                    }
                    let age = replicas
                        .iter()
                        .filter(|replica| {
                            replica.replica_type == *kind
                                && replica.status != ReplicaStatus::Offline
                        })
                        .map(|replica| {
                            (now - replica.last_heartbeat_at).num_seconds().max(0) as u64
                        })
                        .max()
                        .unwrap_or(0);
                    stability::replica_heartbeat_age(kind.as_str(), age);
                }
            }
            Err(err) => {
                succeeded = false;
                warn!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "replica metrics snapshot failed: {err}"
                );
            }
        }
        stability::loop_iteration("operational_metrics", succeeded, started.elapsed());

        tokio::select! {
            _ = shutdown.notified() => return,
            _ = tokio::time::sleep(Duration::from_secs(policy.orchestration.operational_metrics_interval_seconds)) => {}
        }
    }
}

/// periodically mark replicas offline once they have gone quiet past the inactivity window, then
/// hard-delete rows that have stayed quiet far longer so offline replicas do not pile up forever.
/// the operator-facing views derive stale state per fetch; this loop is the durable cleanup that
/// retires replicas that never sent an offline notice (e.g. crashed or evicted pods).
pub async fn run_replica_reaper<T: ReplicaStore>(
    db: Arc<T>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("replica reaper started");
    let registry = ReplicaRegistry::new(db);
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match registry
            .reap_inactive_after(policy.replicas.reap_after_seconds as i64)
            .await
        {
            Ok(count) if count > 0 => {
                stability::cleanup("replica_reap", true, count);
                stability::replica_transition("all", "offline", count);
                info!(count, "reaped inactive replica(s) to offline")
            }
            Ok(_) => {}
            Err(err) => {
                succeeded = false;
                stability::cleanup("replica_reap", false, 1);
                error!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "replica reaper iteration failed: {}", err
                )
            }
        }
        match registry
            .delete_expired_after(policy.replicas.delete_after_seconds as i64)
            .await
        {
            Ok(count) if count > 0 => {
                stability::cleanup("replica_purge", true, count);
                stability::replica_transition("all", "deleted", count);
                info!(count, "purged long-stale replica(s)")
            }
            Ok(_) => {}
            Err(err) => {
                succeeded = false;
                stability::cleanup("replica_purge", false, 1);
                error!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "replica purge iteration failed: {}", err
                )
            }
        }
        match registry
            .prune_samples_after(policy.replicas.sample_retention_seconds as i64)
            .await
        {
            Ok(count) if count > 0 => {
                stability::cleanup("replica_sample_prune", true, count);
                info!(count, "pruned expired replica sample(s)")
            }
            Ok(_) => {}
            Err(err) => {
                succeeded = false;
                stability::cleanup("replica_sample_prune", false, 1);
                error!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "replica sample prune iteration failed: {}", err
                )
            }
        }
        stability::loop_iteration("replica_reaper", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => {
                info!("replica reaper shutting down");
                return;
            }
            _ = tokio::time::sleep(Duration::from_secs(policy.replicas.reaper_interval_seconds)) => {}
        }
    }
}

/// periodically record each org's dedicated node allocation into the usage ledger so per-org
/// node-hours (and cost) can be integrated over time. sampling the recorded allocations keeps
/// accounting exact and provisioner-independent; a missed sample only reduces temporal resolution.
// floor a timestamp to the start of its `interval`-sized window, so instances sampling the same
// window agree on the bucketed `sampled_at` key. falls back to the raw time if the interval is zero.
fn bucket_to_interval(
    now: chrono::DateTime<chrono::Utc>,
    interval: Duration,
) -> chrono::DateTime<chrono::Utc> {
    let secs = interval.as_secs() as i64;
    if secs <= 0 {
        return now;
    }
    let bucketed = now.timestamp() - now.timestamp().rem_euclid(secs);
    chrono::DateTime::from_timestamp(bucketed, 0).unwrap_or(now)
}

#[cfg(test)]
#[path = "loops_tests.rs"]
mod tests;

pub async fn run_workspace_reconciler<
    T: WorkspaceStore + ReplicaStore + OrchestrationStore + IngressStore + RuntimeStore,
>(
    db: Arc<T>,
    publisher: crate::events::EventSender,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("workspace locality reconciler started");
    let operations = WorkspaceOperations::new(db.clone());
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let succeeded = match operations
            .reconcile_expired(
                chrono::Utc::now(),
                None,
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(outcomes) => {
                for outcome in outcomes {
                    match outcome {
                        WorkspaceRecovery::Rebound(workspace) => info!(
                            workspace_id = %workspace.id,
                            worker_instance = %workspace.worker_instance_id,
                            "workspace rebound to returned worker instance"
                        ),
                        WorkspaceRecovery::Waiting(workspace) => info!(
                            workspace_id = %workspace.id,
                            worker_instance = %workspace.worker_instance_id,
                            "workspace waiting for its worker recovery grace"
                        ),
                        WorkspaceRecovery::Abandoned(workspace) => {
                            warn!(
                                workspace_id = %workspace.id,
                                admission_id = %workspace.admission_id,
                                scope = %workspace.scope,
                                attempt = workspace.attempt,
                                "workspace abandoned; notifying its orchestration binding"
                            );
                            if let Err(error) = record_workspace_abandonment(
                                db.as_ref(),
                                &publisher,
                                &workspace,
                                chrono::Utc::now(),
                            )
                            .await
                            {
                                warn!(workspace_id = %workspace.id, error = %error, "failed to enqueue workspace abandonment event");
                            }
                        }
                    }
                }
                match db
                    .fetch_abandoned_workspaces(policy.orchestration.claim_batch_size as i64)
                    .await
                {
                    Ok(workspaces) => {
                        for workspace in workspaces {
                            if let Err(error) = record_workspace_abandonment(
                                db.as_ref(),
                                &publisher,
                                &workspace,
                                chrono::Utc::now(),
                            )
                            .await
                            {
                                warn!(workspace_id = %workspace.id, error = %error, "failed to replay workspace abandonment event");
                            }
                        }
                    }
                    Err(error) => {
                        warn!(error = %error, "failed to scan abandoned workspaces for reducer delivery");
                    }
                }
                if let Err(error) = reconcile_finalizing_workspaces(
                    db.as_ref(),
                    policy.orchestration.claim_batch_size as i64,
                    chrono::Utc::now(),
                )
                .await
                {
                    warn!(error = %error, "failed to reconcile finalizing workspaces");
                }
                true
            }
            Err(error) => {
                warn!(error = %error, "workspace reconciliation failed");
                false
            }
        };
        stability::loop_iteration("workspace_reconciler", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = tokio::time::sleep(Duration::from_secs(policy.orchestration.workspace_reconcile_interval_seconds)) => {}
        }
    }
}

async fn reconcile_finalizing_workspaces<T: WorkspaceStore + ReplicaStore>(
    db: &T,
    limit: i64,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), runinator_models::errors::SendableError> {
    for workspace in db.fetch_finalizing_workspaces(limit).await? {
        let Some(replica_id) = workspace.worker_replica_id else {
            let _ = db
                .transition_workspace_cas(
                    workspace.id,
                    workspace.version,
                    runinator_models::workspaces::WorkspaceStatus::Finalizing,
                    runinator_models::workspaces::WorkspaceStatus::Abandoned,
                    Some(runinator_models::json!({ "reason": "workspace owner was lost during finalization" })),
                    now,
                )
                .await?;
            continue;
        };
        let directives = db.list_agent_directives(replica_id, 200).await?;
        let existing = directives.into_iter().find(|record| {
            matches!(
                &record.kind,
                AgentDirectiveKind::CleanupWorkspace { workspace_id, .. } if *workspace_id == workspace.id
            )
        });
        match existing {
            None => {
                db.enqueue_agent_directive(
                    replica_id,
                    AgentDirectiveKind::CleanupWorkspace {
                        workspace_id: workspace.id,
                        local_key: workspace.local_key.clone(),
                    },
                    now + chrono::Duration::minutes(5),
                )
                .await?;
            }
            Some(record) if record.state == AgentDirectiveState::Completed => {
                let _ = db
                    .transition_workspace_cas(
                        workspace.id,
                        workspace.version,
                        runinator_models::workspaces::WorkspaceStatus::Finalizing,
                        runinator_models::workspaces::WorkspaceStatus::Released,
                        Some(runinator_models::json!({
                            "reason": "worker acknowledged workspace cleanup",
                            "directive_id": record.directive_id,
                            "receipt": record.payload,
                        })),
                        now,
                    )
                    .await?;
            }
            Some(record)
                if matches!(
                    record.state,
                    AgentDirectiveState::Failed
                        | AgentDirectiveState::Unsupported
                        | AgentDirectiveState::Expired
                ) =>
            {
                let _ = db
                    .transition_workspace_cas(
                        workspace.id,
                        workspace.version,
                        runinator_models::workspaces::WorkspaceStatus::Finalizing,
                        runinator_models::workspaces::WorkspaceStatus::Abandoned,
                        Some(runinator_models::json!({
                            "reason": "workspace cleanup could not be acknowledged",
                            "directive_id": record.directive_id,
                            "directive_state": record.state,
                            "message": record.message,
                        })),
                        now,
                    )
                    .await?;
            }
            Some(_) => {}
        }
    }
    Ok(())
}

async fn record_workspace_abandonment<
    T: OrchestrationStore + IngressStore + RuntimeStore + WorkspaceStore,
>(
    db: &T,
    publisher: &crate::events::EventSender,
    workspace: &runinator_models::workspaces::WorkspaceLease,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), runinator_models::errors::SendableError> {
    let Some(binding) = db
        .fetch_orchestration_binding_for_admission(workspace.admission_id, workspace.generation)
        .await?
    else {
        let _ = db
            .mark_workspace_abandonment_notified(workspace.id, workspace.version, now)
            .await?;
        return Ok(());
    };
    if binding.status.is_terminal()
        || !workspace_belongs_to_current_epoch(db, &binding, workspace.id).await?
    {
        let _ = db
            .mark_workspace_abandonment_notified(workspace.id, workspace.version, now)
            .await?;
        return Ok(());
    }
    db.record_ingress_event(
        binding.admission_id,
        binding.generation,
        runinator_models::orchestration::IngressEvent {
            source: "runinator.workspace".into(),
            event_id: format!(
                "workspace:{}:abandoned:v{}",
                workspace.id, workspace.version
            ),
            event_type: "workspace_abandoned".into(),
            correlation_key: binding.correlation_key.clone(),
            payload: runinator_models::json!({
                "workspace_id": workspace.id,
                "scope": workspace.scope,
                "attempt": workspace.attempt,
                "workspace_version": workspace.version,
                "epoch": binding.current_epoch,
                "evidence": workspace.evidence,
            }),
            provenance: Default::default(),
            occurred_at: Some(now),
        },
        runinator_models::orchestration::IngressEventDisposition::Recorded,
        false,
        now,
    )
    .await?;
    let _ = db
        .mark_workspace_abandonment_notified(workspace.id, workspace.version, now)
        .await?;
    crate::events::emit_orchestration(publisher, binding.id, binding.org_id);
    Ok(())
}

async fn workspace_belongs_to_current_epoch<T: OrchestrationStore + RuntimeStore>(
    db: &T,
    binding: &runinator_models::orchestration::OrchestrationBinding,
    workspace_id: uuid::Uuid,
) -> Result<bool, runinator_models::errors::SendableError> {
    let run_id = db
        .fetch_orchestration_epochs(binding.id)
        .await?
        .into_iter()
        .find(|epoch| epoch.epoch == binding.current_epoch)
        .and_then(|epoch| epoch.pipeline_run_id);
    let Some(run) = (match run_id {
        Some(run_id) => db.fetch_pipeline_run(run_id).await?,
        None => None,
    }) else {
        return Ok(false);
    };
    let current = run
        .parameters
        .pointer("/orchestration/workspaces")
        .and_then(Value::as_object)
        .is_some_and(|workspaces| {
            workspaces.values().any(|affinity| {
                affinity
                    .get("workspace_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == workspace_id.to_string())
            })
        });
    Ok(current)
}

pub async fn run_usage_sampler<T: OrgStore>(
    db: Arc<T>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("usage sampler started");
    loop {
        let policy = settings.current();
        let interval = Duration::from_secs(policy.orchestration.usage_sample_interval_seconds);
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match db.list_all_resource_groups().await {
            Ok(groups) => {
                // bucket the timestamp to the sampling-interval boundary so every instance sampling
                // the same window produces the same (org, backend, kind, sampled_at) key; the insert
                // is an idempotent DO-NOTHING upsert, so N-up sampling converges to one row per
                // window instead of over-counting node-hours by the instance count.
                let now = bucket_to_interval(chrono::Utc::now(), interval);
                for group in groups {
                    let org_id = group.org_id;
                    let sample = runinator_models::billing::UsageSample {
                        org_id: group.org_id,
                        backend: group.backend,
                        kind: group.kind,
                        node_count: group.desired,
                        sampled_at: now,
                    };
                    if let Err(err) = db.insert_usage_sample(sample).await {
                        succeeded = false;
                        warn!(
                            org_id = %org_id,
                            error_code = error_code_or_unknown(err.as_ref()),
                            "usage sample insert failed: {}", err
                        );
                    }
                }
            }
            Err(err) => {
                succeeded = false;
                error!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "usage sampler iteration failed: {}", err
                )
            }
        }
        stability::loop_iteration("usage_sampler", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => {
                info!("usage sampler shutting down");
                return;
            }
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

/// periodically turn due workflow triggers into runs (formerly a waker loop, now in-process).
pub async fn run_trigger_loop<
    T: RuntimeStore + DefinitionStore + ScheduleStore + WorkflowVmStore,
>(
    db: Arc<T>,
    events: EventSender,
    instance_id: String,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("trigger firing loop started");
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match repository::claim_due_workflow_trigger_firings(
            db.as_ref(),
            instance_id.clone(),
            policy.orchestration.claim_batch_size as i64,
        )
        .await
        {
            Ok(batch) => {
                let runs = &batch.runs;
                stability::triggers_fired(runs.len() as u64);
                if !runs.is_empty() {
                    info!(count = runs.len(), "fired due workflow trigger(s)");
                }
                // a slot that deliberately produced no run is still worth a line: "the schedule
                // stopped" and "the policy declined" look identical from the run list alone.
                if batch.declined_any() {
                    info!(
                        concurrency_skipped = batch.concurrency_skipped,
                        concurrency_deferred = batch.concurrency_deferred,
                        catchup_skipped = batch.catchup_skipped,
                        "declined due workflow trigger slot(s) by schedule policy"
                    );
                }
                for run_id in &batch.canceled_run_ids {
                    let org_id = repository::org_id_for_workflow_run(db.as_ref(), *run_id).await;
                    emit_workflow_run(&events, *run_id, org_id);
                }
                for run in runs {
                    let org_id = repository::org_id_for_workflow_run(db.as_ref(), run.id).await;
                    emit_workflow_run(&events, run.id, org_id);
                }
                if !runs.is_empty() {
                    // activity tip: unscoped when fired runs span unknown/unowned orgs; individual
                    // run events above carry org when resolvable.
                    emit(
                        &events,
                        crate::events::AppEvent::global(AppEventKind::WorkflowRunActivity),
                    );
                }
            }
            Err(err) => {
                succeeded = false;
                error!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "trigger firing iteration failed: {}", err
                )
            }
        }

        // fire due cron pipeline triggers and start each created pipeline run's entry members.
        match repository::claim_due_pipeline_trigger_firings(
            db.as_ref(),
            instance_id.clone(),
            policy.orchestration.claim_batch_size as i64,
        )
        .await
        {
            Ok(runs) => {
                if !runs.is_empty() {
                    info!(count = runs.len(), "fired due pipeline trigger(s)");
                    for run in &runs {
                        let org_id = repository::org_id_for_pipeline_run(db.as_ref(), run.id).await;
                        emit_pipeline_run(&events, run.id, org_id);
                    }
                    emit(
                        &events,
                        crate::events::AppEvent::global(AppEventKind::PipelineRunActivity),
                    );
                }
            }
            Err(err) => {
                succeeded = false;
                error!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "pipeline trigger firing iteration failed: {}", err
                )
            }
        }
        stability::loop_iteration("trigger_poll", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => {
                info!("trigger firing loop shutting down");
                return;
            }
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.trigger_poll_interval_ms)) => {}
        }
    }
}

/// drain the durable replica-directive outbox, with periodic redelivery as a reconnect backstop.
pub async fn run_agent_directive_publisher<T: ReplicaStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance_id: String,
    agent_nudge: Arc<Notify>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("agent directive publisher started");
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let succeeded = if let Err(err) = repository::publish_due_agent_directives(
            db.as_ref(),
            broker.as_ref(),
            &instance_id,
            policy.orchestration.claim_batch_size as i64,
        )
        .await
        {
            stability::queue_failure("agent_directive", "publish");
            error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "agent directive publisher iteration failed: {err}"
            );
            false
        } else {
            true
        };
        stability::loop_iteration("agent_directive", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = agent_nudge.notified() => {}
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.agent_directive_poll_interval_ms)) => {}
        }
    }
}
