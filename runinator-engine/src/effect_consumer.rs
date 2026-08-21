//! Settlement loop for the workflow VM's generic effect protocol.

use std::sync::Arc;

use runinator_broker_core::Broker;
use runinator_comm::EffectResultKind;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::workflow_vm::{WorkflowEffectOutput, WorkflowEffectOutputEvent};
use runinator_models::{
    notifications::NotificationDeliveryStatus, workflow_vm::WorkflowEffectStatus,
};
use tokio::sync::Notify;
use tracing::{error, info, warn};

const EFFECT_RESULT_CONSUMER_ID: &str = "runinator-ws-effects";

/// Consume effect results independently of the legacy node-run result channel. A stale attempt is
/// harmless: `settle_workflow_effect` returns `false`, after which this delivery is acknowledged.
pub async fn run_effect_result_consumer<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    publisher: crate::events::EnginePublisher,
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
                // Notification sends have no stream/artifact contract; acknowledge stray payloads
                // so an old/misbehaving provider cannot wedge the result channel.
                EffectResultKind::Chunk { .. } | EffectResultKind::Artifact { .. } => Ok(()),
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
