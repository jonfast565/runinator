//! Consumption loop for the ingress channel.
//!
//! Ingress is the one direction that runs *toward* the engine: the waker relays a due timer wake
//! here, a worker raises a control request from inside an executing action, and a desktop agent
//! reports the outcome of a durable fleet directive. Producers therefore never consume their own
//! messages, and this loop is the sole consumer.

use std::{sync::Arc, time::Duration};

use runinator_broker_core::{Broker, EffectResultMessage, IngressDelivery};
use runinator_comm::{ControlKind, WsIngressCommand};
use runinator_models::errors::SendableError;
use runinator_store::{
    RuntimeStore,
    roles::{ReplicaStore, WorkflowVmStore},
};
use tokio::sync::Notify;
use tracing::{error, info, warn};

const INGRESS_CONSUMER_ID: &str = "runinator-engine-ingress";

/// Apply ingress messages until shutdown.
///
/// A message is acknowledged once its effect has been durably recorded or handed to the channel
/// that owns it; anything else is returned to the broker, since dropping an ingress message loses
/// the only copy of a due timer or a directive reply.
pub async fn run_ingress_consumer<T: RuntimeStore + ReplicaStore + WorkflowVmStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    shutdown: Arc<Notify>,
) {
    run_ingress_consumer_with_orchestration_nudge(db, broker, Arc::new(Notify::new()), shutdown)
        .await;
}

pub async fn run_ingress_consumer_with_orchestration_nudge<
    T: RuntimeStore + ReplicaStore + WorkflowVmStore,
>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    orchestration_nudge: Arc<Notify>,
    shutdown: Arc<Notify>,
) {
    info!("workflow ingress consumer started");
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => return,
            received = broker.receive_ingress(INGRESS_CONSUMER_ID) => match received {
                Ok(delivery) => delivery,
                Err(err) => {
                    error!(error = %err, "failed to receive ingress message");
                    tokio::select! {
                        _ = shutdown.notified() => return,
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                    continue;
                }
            }
        };

        match apply(
            db.clone(),
            broker.as_ref(),
            orchestration_nudge.as_ref(),
            &delivery,
        )
        .await
        {
            Ok(()) => {
                crate::stability::ingress_applied();
                if let Err(err) = broker
                    .ack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    warn!(error = %err, "failed to ack ingress message");
                }
            }
            Err(err) => {
                crate::stability::ingress_retried();
                error!(error = %err, "failed to apply ingress message");
                if let Err(err) = broker
                    .nack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    warn!(error = %err, "failed to requeue ingress message");
                }
            }
        }
    }
}

async fn apply<T: RuntimeStore + ReplicaStore + WorkflowVmStore>(
    db: Arc<T>,
    broker: &dyn Broker,
    orchestration_nudge: &Notify,
    delivery: &IngressDelivery,
) -> Result<(), SendableError> {
    match &delivery.command {
        // a timer came due. the result was built by the infrastructure effect host that armed the
        // wake, so this republishes it on the ordinary effect-result channel rather than settling
        // it here: retry policy, interrupt handling, and run events all live in that one consumer,
        // and a second settle path would have to duplicate every one of them.
        WsIngressCommand::SettleEffect { result, trace_id } => {
            info!(
                effect_id = %result.effect_id,
                workflow_run_id = %result.workflow_run_id,
                trace_id = %trace_id,
                "relaying a due timer wake to the effect-result channel",
            );
            match broker
                .publish_effect_result(EffectResultMessage {
                    dedupe_key: Some(result.event_id.to_string()),
                    result: result.clone(),
                    enqueued_at: chrono::Utc::now(),
                })
                .await
            {
                // the same wake relayed twice settles the same effect twice, which the settle
                // itself already rejects; treat the duplicate as done.
                Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => Ok(()),
                Err(err) => Err(crate::errors::EFFECT_RESULT_PUBLISH.error(err)),
            }
        }
        WsIngressCommand::TimerInterrupt {
            timer,
            due_at,
            trace_id,
        } => {
            info!(
                workflow_run_id = %timer.workflow_run_id,
                timer_id = %timer.timer_id,
                due_at = %due_at,
                trace_id = %trace_id,
                "applying due workflow timer interrupt"
            );
            db.fire_workflow_timer_interrupt(
                runinator_store::roles::WorkflowTimerInterrupt {
                    workflow_run_id: timer.workflow_run_id,
                    timer_id: timer.timer_id.clone(),
                    interval_seconds: timer.interval_seconds,
                    due_at: *due_at,
                },
                chrono::Utc::now(),
            )
            .await?;
            Ok(())
        }
        WsIngressCommand::OrchestrationIntent {
            wake,
            due_at,
            trace_id,
        } => {
            info!(
                binding_id = %wake.binding_id,
                intent = %wake.intent,
                due_at = %due_at,
                trace_id = %trace_id,
                "nudging the correlated orchestration reducer",
            );
            orchestration_nudge.notify_one();
            Ok(())
        }
        // a worker asked for run control from inside an executing action.
        WsIngressCommand::Control {
            workflow_run_id,
            kind,
        } => {
            info!(workflow_run_id = %workflow_run_id, kind = ?kind, "applying a worker control request");
            match kind {
                ControlKind::Cancel => {
                    crate::repository::cancel_workflow_run(db.as_ref(), broker, *workflow_run_id)
                        .await?;
                }
                ControlKind::Pause => {
                    crate::repository::pause_workflow_run(db.as_ref(), *workflow_run_id).await?;
                }
                ControlKind::Resume => {
                    crate::repository::resume_workflow_run(db.as_ref(), *workflow_run_id).await?;
                }
                // Terminal controls are worker-local and are never valid on ingress. Ignore a
                // malformed or mixed-version message rather than changing workflow state.
                ControlKind::Terminal => {}
            }
            Ok(())
        }
        // an agent finished (or refused) a durable directive. recording it is what moves the
        // directive out of its issued state, so a dropped reply would leave a drain or restart
        // pending forever.
        WsIngressCommand::AgentDirectiveResult { result } => {
            let directive_id = result.directive_id;
            let record =
                crate::repository::complete_agent_directive(db.as_ref(), result.clone()).await?;
            if record.is_none() {
                // an unknown or already-completed directive is not a failure: a redelivered reply
                // must not park the ingress channel behind a message that can never apply.
                warn!(directive_id = %directive_id, "ignoring a reply for an unknown agent directive");
            }
            Ok(())
        }
        WsIngressCommand::ReplicaAvailability { availability } => {
            info!("applying broker-announced replica availability");
            crate::services::ReplicaRegistry::new(db)
                .observe_broker_availability(availability.clone())
                .await
        }
    }
}

#[cfg(test)]
#[path = "ingress_consumer_tests.rs"]
mod tests;
