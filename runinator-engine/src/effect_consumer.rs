//! Settlement loop for the workflow VM's generic effect protocol.

use std::sync::Arc;

use runinator_adapter_contract::AdapterPollResponse;
use runinator_broker_core::Broker;
use runinator_comm::EffectResultKind;
use runinator_models::interrupt::InterruptSource;
use runinator_models::workflow_vm::{
    WorkflowEffectOutput, WorkflowEffectOutputEvent, WorkflowJournalEntry,
};
use runinator_models::{
    notifications::NotificationDeliveryStatus,
    orchestration::{DeliverySemantics, ExternalOperationStatus, OrchestrationEvidence},
    workflow_vm::WorkflowEffectStatus,
};
use runinator_runtime::workflow_vm::interrupt_handler_continuation;
use runinator_store::roles::{ExternalOperationUpdate, OrchestrationStore, WorkflowVmStore};
use tokio::sync::Notify;
use tracing::{error, info, warn};

const EFFECT_RESULT_CONSUMER_ID: &str = "runinator-ws-effects";

/// Consume effect results independently of the legacy node-run result channel. A stale attempt is
/// harmless: `settle_workflow_effect` returns `false`, after which this delivery is acknowledged.
pub async fn run_effect_result_consumer<T: crate::engine::BackgroundEngineStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    publisher: crate::events::EventSender,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM effect result consumer started");
    loop {
        let mut delivery = tokio::select! {
            _ = shutdown.notified() => return,
            received = broker.receive_effect_result(EFFECT_RESULT_CONSUMER_ID) => match received {
                Ok(delivery) => delivery,
                Err(err) => {
                    error!(error = %err, "failed to receive workflow VM effect result");
                    tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {} }
                    continue;
                }
            }
        };

        // Notification commands use the same provider-effect transport but are not VM effects:
        // their durable owner is the notification delivery row, so never attempt a continuation
        // settlement for them.
        if let Some(notification_delivery_id) = delivery.result.notification_delivery_id {
            let result = match &delivery.result.kind {
                EffectResultKind::Status {
                    status, message, ..
                } => {
                    let status = if *status == WorkflowEffectStatus::Succeeded {
                        NotificationDeliveryStatus::Delivered
                    } else {
                        NotificationDeliveryStatus::Failed
                    };
                    db.mark_notification_delivery(notification_delivery_id, status, message.clone())
                        .await
                }
                // Notification sends have no stream/artifact/lease contract; acknowledge stray
                // payloads so an old/misbehaving provider cannot wedge the result channel.
                EffectResultKind::Chunk { .. }
                | EffectResultKind::Artifact { .. }
                | EffectResultKind::Progress { .. }
                | EffectResultKind::TerminalInteraction { .. }
                | EffectResultKind::Claimed { .. } => Ok(()),
            };
            match result {
                Ok(()) => {
                    if let Err(err) = broker
                        .ack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        warn!(error = %err, "failed to ack notification effect result");
                    }
                }
                Err(err) => {
                    error!(error = %err, notification_delivery_id = %notification_delivery_id, "failed to settle notification delivery");
                    if let Err(err) = broker
                        .nack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        warn!(error = %err, "failed to requeue notification effect result");
                    }
                }
            }
            continue;
        }

        let adapter_dispatch = match db
            .fetch_orchestration_adapter_poll_dispatch(delivery.result.effect_id)
            .await
        {
            Ok(value) => value,
            Err(err) => {
                error!(error = %err, effect_id = %delivery.result.effect_id, "failed to classify adapter poll result");
                let _ = broker
                    .nack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                    .await;
                continue;
            }
        };
        if let Some(dispatch) = adapter_dispatch {
            let settled = settle_adapter_poll_result(
                db.clone(),
                broker.clone(),
                publisher.clone(),
                &dispatch,
                &delivery.result.kind,
            )
            .await;
            match settled {
                Ok(()) => {
                    if let Err(err) = broker
                        .ack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        warn!(error = %err, "failed to ack adapter poll result");
                    }
                }
                Err(err) => {
                    error!(error = %err, effect_id = %delivery.result.effect_id, "failed to settle adapter poll result");
                    if let Err(err) = broker
                        .nack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        warn!(error = %err, "failed to requeue adapter poll result");
                    }
                }
            }
            continue;
        }

        // A timeout cannot prove whether an at-least-once provider committed its side effect.
        // Keep the continuation parked until an operator records the observed outcome; settling
        // it here would let the graph advance while the ambiguity still exists.
        let hold_ambiguity = match hold_ambiguous_at_least_once(db.as_ref(), &delivery.result).await
        {
            Ok(hold) => hold,
            Err(err) => {
                error!(error = %err, effect_id = %delivery.result.effect_id, "failed to classify external operation ambiguity");
                if let Err(err) = broker
                    .nack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    warn!(error = %err, "failed to requeue unclassified external operation result");
                }
                continue;
            }
        };
        if hold_ambiguity {
            match record_external_operation_result(db.as_ref(), &delivery.result).await {
                Ok(changed) => {
                    emit_external_operation_change(db.as_ref(), &publisher, changed).await;
                    if let Err(err) = broker
                        .ack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        warn!(error = %err, "failed to ack ambiguous external operation result");
                    }
                }
                Err(err) => {
                    error!(error = %err, effect_id = %delivery.result.effect_id, "failed to record ambiguous external operation result");
                    if let Err(err) = broker
                        .nack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        warn!(error = %err, "failed to requeue ambiguous external operation result");
                    }
                }
            }
            continue;
        }

        if let Some(commit) = &delivery.result.workspace_commit
            && (commit
                .snapshot
                .revision_id
                .parse::<runinator_workspace::storage::Id>()
                .is_err()
                || commit.checkout.effect_id != delivery.result.effect_id
                || commit.receipt_id.is_nil())
        {
            let error = runinator_models::errors::WORKSPACE_INVALID
                .error("invalid workspace receipt reference");
            delivery.result.kind = EffectResultKind::Status {
                status: WorkflowEffectStatus::Rejected,
                output: None,
                message: Some(error.to_string()),
            };
            delivery.result.workspace_commit = None;
        }
        let settled = match &delivery.result.kind {
            EffectResultKind::Status {
                status,
                output,
                message,
            } => {
                // A retryable terminal re-arms the effect instead of settling it: the continuation
                // stays parked on the same effect, so the graph never sees the failed attempt.
                match schedule_retry(
                    db.as_ref(),
                    delivery.result.effect_id,
                    delivery.result.attempt,
                    *status,
                    message.clone(),
                    delivery.result.timestamp,
                )
                .await
                {
                    Ok(true) => Ok(true),
                    Ok(false) => {
                        let completed = db
                            .settle_workflow_effect_with_workspace(
                                runinator_store::roles::workflow_vm::WorkspaceEffectSettlement {
                                    effect_id: delivery.result.effect_id,
                                    attempt: delivery.result.attempt,
                                    status: *status,
                                    output: output.clone(),
                                    message: message.clone(),
                                    settled_at: delivery.result.timestamp,
                                    workspace: delivery.result.workspace_commit.as_deref().cloned(),
                                },
                            )
                            .await;
                        match completed {
                            Err(error)
                                if error
                                    .downcast_ref::<runinator_models::errors::RuntimeError>()
                                    .and_then(|error| error.numbered_code())
                                    == Some("WORKSPACE002") =>
                            {
                                db.settle_workflow_effect(
                                    delivery.result.effect_id,
                                    delivery.result.attempt,
                                    WorkflowEffectStatus::Rejected,
                                    None,
                                    Some(error.to_string()),
                                    delivery.result.timestamp,
                                )
                                .await
                            }
                            result => result,
                        }
                    }
                    Err(err) => Err(err),
                }
            }
            EffectResultKind::Chunk { stream, content } => {
                let output = WorkflowEffectOutput::Chunk {
                    stream: stream.clone(),
                    content: content.clone(),
                };
                db.append_workflow_effect_output(WorkflowEffectOutputEvent {
                    event_id: delivery.result.event_id,
                    effect_id: delivery.result.effect_id,
                    workflow_run_id: delivery.result.workflow_run_id,
                    continuation_id: delivery.result.continuation_id,
                    attempt: delivery.result.attempt,
                    timeline_category: output.timeline_category(),
                    output,
                    created_at: delivery.result.timestamp.timestamp(),
                })
                .await
            }
            EffectResultKind::Claimed {
                executor_replica_id,
            } => {
                db.claim_workflow_effect_executor(
                    delivery.result.effect_id,
                    delivery.result.attempt,
                    *executor_replica_id,
                    delivery.result.timestamp,
                )
                .await
            }
            EffectResultKind::Artifact { artifact } => {
                let output = WorkflowEffectOutput::Artifact {
                    artifact: artifact.clone(),
                };
                db.append_workflow_effect_output(WorkflowEffectOutputEvent {
                    event_id: delivery.result.event_id,
                    effect_id: delivery.result.effect_id,
                    workflow_run_id: delivery.result.workflow_run_id,
                    continuation_id: delivery.result.continuation_id,
                    attempt: delivery.result.attempt,
                    timeline_category: output.timeline_category(),
                    output,
                    created_at: delivery.result.timestamp.timestamp(),
                })
                .await
            }
            EffectResultKind::Progress { kind, payload } => {
                let output = WorkflowEffectOutput::Progress {
                    kind: kind.clone(),
                    payload: payload.clone(),
                };
                db.append_workflow_effect_output(WorkflowEffectOutputEvent {
                    event_id: delivery.result.event_id,
                    effect_id: delivery.result.effect_id,
                    workflow_run_id: delivery.result.workflow_run_id,
                    continuation_id: delivery.result.continuation_id,
                    attempt: delivery.result.attempt,
                    timeline_category: output.timeline_category(),
                    output,
                    created_at: delivery.result.timestamp.timestamp(),
                })
                .await
            }
            EffectResultKind::TerminalInteraction { interaction } => {
                let output = WorkflowEffectOutput::TerminalInteraction {
                    interaction: interaction.clone(),
                };
                db.record_workflow_terminal_interaction(
                    WorkflowEffectOutputEvent {
                        event_id: delivery.result.event_id,
                        effect_id: delivery.result.effect_id,
                        workflow_run_id: delivery.result.workflow_run_id,
                        continuation_id: delivery.result.continuation_id,
                        attempt: delivery.result.attempt,
                        timeline_category: output.timeline_category(),
                        output,
                        created_at: delivery.result.timestamp.timestamp(),
                    },
                    delivery.result.timestamp,
                )
                .await
            }
        };

        match settled {
            Ok(applied) => {
                match record_external_operation_result(db.as_ref(), &delivery.result).await {
                    Ok(changed) => {
                        emit_external_operation_change(db.as_ref(), &publisher, changed).await;
                    }
                    Err(err) => {
                        error!(error = %err, effect_id = %delivery.result.effect_id, "failed to record external operation receipt");
                        if let Err(err) = broker
                            .nack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                            .await
                        {
                            warn!(error = %err, "failed to requeue external operation receipt");
                        }
                        continue;
                    }
                }
                if applied {
                    info!(effect_id = %delivery.result.effect_id, "settled workflow VM effect");
                    crate::events::emit_workflow_run_resolved(
                        db.as_ref(),
                        &publisher,
                        delivery.result.workflow_run_id,
                    )
                    .await;
                }
                if let Err(err) = broker
                    .ack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    warn!(error = %err, "failed to ack workflow VM effect result");
                }
            }
            Err(err) => {
                error!(error = %err, effect_id = %delivery.result.effect_id, "failed to settle workflow VM effect");
                if let Err(err) = broker
                    .nack_effect_result(EFFECT_RESULT_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    warn!(error = %err, "failed to requeue workflow VM effect result");
                }
            }
        }
    }
}

pub(crate) async fn settle_adapter_poll_result<T: crate::engine::BackgroundEngineStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    publisher: crate::events::EventSender,
    dispatch: &runinator_store::roles::AdapterPollDispatch,
    kind: &EffectResultKind,
) -> Result<(), String> {
    if matches!(dispatch.state.as_str(), "succeeded" | "failed" | "expired") {
        return Ok(());
    }
    if dispatch.deadline_at <= chrono::Utc::now() {
        db.finish_adapter_poll_attempt(
            dispatch.id,
            "expired".into(),
            Default::default(),
            Some("late poll result ignored".into()),
            chrono::Utc::now(),
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(());
    }
    if matches!(kind, EffectResultKind::Claimed { .. }) {
        db.update_orchestration_adapter_poll_dispatch_state(
            dispatch.id,
            "running".into(),
            chrono::Utc::now(),
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(());
    }
    let EffectResultKind::Status {
        status,
        output,
        message,
    } = kind
    else {
        return Ok(());
    };
    if dispatch.dry_run {
        let value: serde_json::Value = output.clone().unwrap_or_default().into();
        let mut result = serde_json::Value::Null;
        let mut error = message.clone();
        if *status == WorkflowEffectStatus::Succeeded {
            match serde_json::from_value::<AdapterPollResponse>(value) {
                Ok(response) => {
                    let operations = crate::services::AdapterOperations::new(db.clone());
                    let adapter = operations
                        .fetch(dispatch.adapter_id)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| "adapter no longer exists".to_string())?;
                    let mut previews = Vec::new();
                    for event in &response.events {
                        previews.push(match operations.preview_event(&adapter, event).await {
                            Ok(value) => value,
                            Err(error) => {
                                serde_json::json!({"validation_errors":[error.to_string()]})
                            }
                        });
                    }
                    error = response.error.clone();
                    result = serde_json::json!({"verified":error.is_none(),"events":response.events,"previews":previews,"errors":error.iter().collect::<Vec<_>>()});
                }
                Err(cause) => error = Some(cause.to_string()),
            }
        } else if error.is_none() {
            error = Some(format!("poll test returned {status:?}"));
        }
        db.finish_adapter_poll_attempt(
            dispatch.id,
            if error.is_some() {
                "failed"
            } else {
                "succeeded"
            }
            .into(),
            result.into(),
            error,
            chrono::Utc::now(),
        )
        .await
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let failure = if *status == WorkflowEffectStatus::Succeeded {
        let value: serde_json::Value = output.clone().unwrap_or_default().into();
        match serde_json::from_value::<AdapterPollResponse>(value) {
            Ok(response) => {
                let pipelines =
                    crate::services::PipelineOperations::new(db.clone(), broker, publisher, None);
                crate::adapter_polling::settle_dispatched_poll(
                    db.clone(),
                    &pipelines,
                    dispatch,
                    response,
                )
                .await
                .err()
            }
            Err(error) => Some(format!(
                "adapter worker returned an invalid response: {error}"
            )),
        }
    } else {
        Some(
            message
                .clone()
                .unwrap_or_else(|| format!("adapter worker returned {status:?}")),
        )
    };
    let now = chrono::Utc::now();
    if let Some(error) = failure {
        db.fail_orchestration_adapter_poll(
            dispatch.adapter_id,
            dispatch.claim_owner.clone(),
            now + chrono::TimeDelta::seconds(60),
            error.clone(),
            now,
        )
        .await
        .map_err(|error| error.to_string())?;
        db.finish_adapter_poll_attempt(
            dispatch.id,
            "failed".into(),
            Default::default(),
            Some(error.clone()),
            now,
        )
        .await
        .map_err(|error| error.to_string())?;
    } else {
        db.finish_adapter_poll_attempt(
            dispatch.id,
            "succeeded".into(),
            Default::default(),
            None,
            now,
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn hold_ambiguous_at_least_once<T: OrchestrationStore>(
    db: &T,
    result: &runinator_comm::EffectResult,
) -> Result<bool, runinator_models::errors::SendableError> {
    if !matches!(
        &result.kind,
        EffectResultKind::Status {
            status: WorkflowEffectStatus::TimedOut,
            ..
        }
    ) {
        return Ok(false);
    }
    Ok(db
        .fetch_external_operation_for_effect(result.effect_id)
        .await?
        .is_some_and(|operation| operation.semantics == DeliverySemantics::AtLeastOnce))
}

async fn record_external_operation_result<T: OrchestrationStore>(
    db: &T,
    result: &runinator_comm::EffectResult,
) -> Result<Option<(uuid::Uuid, uuid::Uuid)>, runinator_models::errors::SendableError> {
    let Some(operation) = db
        .fetch_external_operation_for_effect(result.effect_id)
        .await?
    else {
        return Ok(None);
    };
    if operation.attempt > i64::from(result.attempt) {
        return Ok(None);
    }
    let current = db
        .fetch_current_orchestration_binding_for_workflow_run(result.workflow_run_id)
        .await?;
    let stale = current.as_ref().is_none_or(|binding| {
        binding.id != operation.binding_id || binding.current_epoch != operation.epoch
    });
    let (status, ambiguous, receipt) = match &result.kind {
        EffectResultKind::Claimed {
            executor_replica_id,
        } => (
            ExternalOperationStatus::Running,
            false,
            runinator_models::json!({
                "kind": "claimed",
                "executor_replica_id": executor_replica_id,
                "event_id": result.event_id,
                "stale": stale,
            }),
        ),
        EffectResultKind::Status {
            status,
            output,
            message,
        } => {
            let ambiguous = matches!(status, WorkflowEffectStatus::TimedOut);
            let operation_status = if *status == WorkflowEffectStatus::Succeeded {
                ExternalOperationStatus::Succeeded
            } else if ambiguous && operation.semantics == DeliverySemantics::AtLeastOnce {
                ExternalOperationStatus::Waiting
            } else {
                ExternalOperationStatus::Failed
            };
            (
                operation_status,
                ambiguous,
                runinator_models::json!({
                    "kind": "status",
                    "status": status,
                    "output": output,
                    "message": message,
                    "event_id": result.event_id,
                    "stale": stale,
                }),
            )
        }
        EffectResultKind::Chunk { .. }
        | EffectResultKind::Artifact { .. }
        | EffectResultKind::Progress { .. }
        | EffectResultKind::TerminalInteraction { .. } => return Ok(None),
    };
    let updated = db
        .update_external_operation(
            operation.id,
            ExternalOperationUpdate {
                status,
                attempt: i64::from(result.attempt),
                ambiguous,
                provenance: operation.provenance.clone(),
                receipt: receipt.clone(),
            },
            result.timestamp,
        )
        .await?;
    if stale && updated.is_some() {
        db.append_orchestration_evidence(OrchestrationEvidence {
            id: uuid::Uuid::now_v7(),
            binding_id: operation.binding_id,
            epoch: Some(operation.epoch),
            kind: "stale_external_operation_receipt".into(),
            subject_revision: None,
            payload: receipt,
            source_event_id: None,
            created_at: result.timestamp,
        })
        .await?;
    }
    Ok(updated.map(|operation| (operation.id, operation.binding_id)))
}

async fn emit_external_operation_change<T: OrchestrationStore>(
    db: &T,
    publisher: &crate::events::EventSender,
    changed: Option<(uuid::Uuid, uuid::Uuid)>,
) {
    let Some((operation_id, binding_id)) = changed else {
        return;
    };
    if let Ok(Some(binding)) = db.fetch_orchestration_binding(binding_id).await {
        crate::events::emit_external_operation(publisher, operation_id, binding_id, binding.org_id);
    }
}

/// Re-arm `effect_id` under its node's retry policy, returning whether it was re-armed.
///
/// A non-terminal status, a non-retryable terminal, or an exhausted budget all return `false`, at
/// which point the caller settles the effect exactly as it did before retries existed.
async fn schedule_retry<T: WorkflowVmStore>(
    db: &T,
    effect_id: uuid::Uuid,
    attempt: u32,
    status: WorkflowEffectStatus,
    message: Option<String>,
    settled_at: chrono::DateTime<chrono::Utc>,
) -> Result<bool, runinator_models::errors::SendableError> {
    if !status.is_terminal() {
        return Ok(false);
    }
    let Some(effect) = db.fetch_workflow_effect(effect_id).await? else {
        return Ok(false);
    };
    // guard the stale attempt here too: settling would reject it, and a retry must not be the one
    // path where a duplicate late result gets to schedule extra work.
    if effect.attempt != attempt {
        return Ok(false);
    }
    let Some(available_at) = crate::effect_retry::next_attempt_at(&effect, status, settled_at)
    else {
        return Ok(false);
    };
    let retried = db
        .retry_workflow_effect(
            effect_id,
            attempt,
            available_at,
            message.clone(),
            settled_at,
        )
        .await?;
    if !retried {
        return Ok(false);
    }
    info!(
        effect_id = %effect_id,
        attempt = attempt + 1,
        available_at = %available_at,
        "re-arming failed effect under its retry policy"
    );
    // fail-open, exactly like every other interrupt source: a handler that cannot be started must
    // never stop the retry it was only observing.
    if let Err(err) = raise_retry_interrupt(db, &effect, attempt, available_at, message).await {
        warn!(error = %err, effect_id = %effect_id, "failed to raise the retry interrupt handler");
    }
    Ok(true)
}

/// Run a declared `interrupt on retry` handler beside the thread parked on `effect`.
///
/// The parked thread is deliberately left `Waiting` rather than suspended: it is already stopped,
/// and suspending it would stop the retried effect from ever settling it. The handler therefore
/// observes the retry and hands nothing back — its `resume` retires it and leaves the main flow to
/// the attempt now queued.
async fn raise_retry_interrupt<T: WorkflowVmStore>(
    db: &T,
    effect: &runinator_models::workflow_vm::WorkflowEffect,
    attempt: u32,
    available_at: chrono::DateTime<chrono::Utc>,
    message: Option<String>,
) -> Result<(), runinator_models::errors::SendableError> {
    let Some(module) = db.fetch_workflow_module(effect.workflow_run_id).await? else {
        return Ok(());
    };
    let Some(declared) = module.interrupt_handler(InterruptSource::Retry) else {
        return Ok(());
    };
    let target = declared.target;
    let Some(continuation) = db
        .fetch_workflow_continuation(effect.continuation_id)
        .await?
    else {
        return Ok(());
    };
    let payload = runinator_models::json!({
        "effect_id": effect.id,
        "node_id": effect.node_id,
        "failed_attempt": attempt,
        "next_attempt": attempt + 1,
        "next_attempt_at": available_at.timestamp(),
        "message": message,
    });
    let handler = interrupt_handler_continuation(
        &module,
        &continuation,
        InterruptSource::Retry,
        payload,
        target,
        // one handler per attempt: without this the stable id would suppress every retry after the
        // first.
        &format!("attempt:{}", attempt + 1),
    );
    let journal = WorkflowJournalEntry::Interrupted {
        continuation_id: continuation.id,
        handler_continuation_id: handler.id,
        source: InterruptSource::Retry,
    };
    db.start_workflow_interrupt_handler(handler, journal).await
}
