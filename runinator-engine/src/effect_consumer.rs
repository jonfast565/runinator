//! Settlement loop for the workflow VM's generic effect protocol.

use std::sync::Arc;

use runinator_broker_core::Broker;
use runinator_comm::EffectResultKind;
use runinator_models::interrupt::InterruptSource;
use runinator_models::workflow_vm::{
    WorkflowEffectOutput, WorkflowEffectOutputEvent, WorkflowJournalEntry,
};
use runinator_models::{
    notifications::NotificationDeliveryStatus, workflow_vm::WorkflowEffectStatus,
};
use runinator_runtime::workflow_vm::interrupt_handler_continuation;
use runinator_store::{
    RuntimeStore,
    roles::{NotificationStore, WorkflowVmStore},
};
use tokio::sync::Notify;
use tracing::{error, info, warn};

const EFFECT_RESULT_CONSUMER_ID: &str = "runinator-ws-effects";

/// Consume effect results independently of the legacy node-run result channel. A stale attempt is
/// harmless: `settle_workflow_effect` returns `false`, after which this delivery is acknowledged.
pub async fn run_effect_result_consumer<T: WorkflowVmStore + NotificationStore + RuntimeStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    publisher: crate::events::EventSender,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM effect result consumer started");
    loop {
        let delivery = tokio::select! {
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
                        db.settle_workflow_effect(
                            delivery.result.effect_id,
                            delivery.result.attempt,
                            status.clone(),
                            output.clone(),
                            message.clone(),
                            delivery.result.timestamp,
                        )
                        .await
                    }
                    Err(err) => Err(err),
                }
            }
            EffectResultKind::Chunk { stream, content } => {
                db.append_workflow_effect_output(WorkflowEffectOutputEvent {
                    event_id: delivery.result.event_id,
                    effect_id: delivery.result.effect_id,
                    workflow_run_id: delivery.result.workflow_run_id,
                    continuation_id: delivery.result.continuation_id,
                    attempt: delivery.result.attempt,
                    output: WorkflowEffectOutput::Chunk {
                        stream: stream.clone(),
                        content: content.clone(),
                    },
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
                db.append_workflow_effect_output(WorkflowEffectOutputEvent {
                    event_id: delivery.result.event_id,
                    effect_id: delivery.result.effect_id,
                    workflow_run_id: delivery.result.workflow_run_id,
                    continuation_id: delivery.result.continuation_id,
                    attempt: delivery.result.attempt,
                    output: WorkflowEffectOutput::Artifact {
                        artifact: artifact.clone(),
                    },
                    created_at: delivery.result.timestamp.timestamp(),
                })
                .await
            }
        };

        match settled {
            Ok(applied) => {
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
