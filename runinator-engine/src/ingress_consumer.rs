//! Consumption loop for the ingress channel.
//!
//! Ingress is the one direction that runs *toward* the engine: the waker relays a due timer wake
//! here, a worker raises a control request from inside an executing action, and a desktop agent
//! reports the outcome of a durable fleet directive. Producers therefore never consume their own
//! messages, and this loop is the sole consumer.

use std::{sync::Arc, time::Duration};

use runinator_broker_core::{Broker, EffectResultMessage, IngressDelivery};
use runinator_comm::{ControlKind, WsIngressCommand};
use runinator_models::{
    auth::ResourceType,
    errors::SendableError,
    ingress_control::{
        BrokerIngressCapture, BrokerIngressCaptureRequest, BrokerIngressSessionMode,
        INGRESS_CONTROL_QUEUE_CAPACITY, IngressControlState,
    },
    rbac::{ScopeKind, ScopeRef},
    value::Value,
};
use runinator_store::{
    RuntimeStore,
    roles::{DeliveryStore, OrchestrationStore, RbacStore, ReplicaStore, WorkflowVmStore},
};
use tokio::sync::Notify;
use tracing::{error, info, warn};

const INGRESS_CONSUMER_ID: &str = "runinator-engine-ingress";

/// Apply ingress messages until shutdown.
///
/// A message is acknowledged once its effect has been durably recorded or handed to the channel
/// that owns it; anything else is returned to the broker, since dropping an ingress message loses
/// the only copy of a due timer or a directive reply.
pub async fn run_ingress_consumer<
    T: RuntimeStore + ReplicaStore + WorkflowVmStore + DeliveryStore + RbacStore + OrchestrationStore,
>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    shutdown: Arc<Notify>,
) {
    run_ingress_consumer_with_orchestration_nudge(db, broker, Arc::new(Notify::new()), shutdown)
        .await;
}

pub async fn run_ingress_consumer_with_orchestration_nudge<
    T: RuntimeStore + ReplicaStore + WorkflowVmStore + DeliveryStore + RbacStore + OrchestrationStore,
>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    orchestration_nudge: Arc<Notify>,
    shutdown: Arc<Notify>,
) {
    info!("workflow ingress consumer started");
    let mut approvals = tokio::time::interval(Duration::from_millis(250));
    let mut last_cleanup = chrono::Utc::now();
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => return,
            _ = approvals.tick() => {
                match db.claim_approved_broker_ingress(chrono::Utc::now()).await {
                    Ok(Some(record)) => {
                        match serde_json::from_value::<WsIngressCommand>(record.command.clone().into()) {
                            Ok(command) => {
                                let result = apply_command(db.clone(), broker.as_ref(), orchestration_nudge.as_ref(), &command).await;
                                let (state, error) = match result {
                                    Ok(()) => (IngressControlState::Applied, None),
                                    Err(error) => (IngressControlState::Failed, Some(error.to_string())),
                                };
                                if let Err(error) = db.finish_broker_ingress_record(record.id, state, error, chrono::Utc::now()).await {
                                    warn!(error = %error, record_id = %record.id, "failed to settle approved broker ingress record");
                                }
                            }
                            Err(error) => {
                                let _ = db.finish_broker_ingress_record(record.id, IngressControlState::Failed, Some(error.to_string()), chrono::Utc::now()).await;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(error) => warn!(error = %error, "failed to claim approved broker ingress record"),
                }
                if chrono::Utc::now() - last_cleanup >= chrono::Duration::minutes(1) {
                    let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
                    if let Err(error) = db.purge_broker_ingress_records_before(cutoff).await {
                        warn!(error = %error, "failed to purge expired broker ingress records");
                    }
                    last_cleanup = chrono::Utc::now();
                }
                continue;
            }
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

        match inspect_or_apply(
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

fn command_kind(command: &WsIngressCommand) -> &'static str {
    match command {
        WsIngressCommand::SettleEffect { .. } => "settle_effect",
        WsIngressCommand::TimerInterrupt { .. } => "timer_interrupt",
        WsIngressCommand::OrchestrationIntent { .. } => "orchestration_intent",
        WsIngressCommand::Control { .. } => "control",
        WsIngressCommand::AgentDirectiveResult { .. } => "agent_directive_result",
        WsIngressCommand::ReplicaAvailability { .. } => "replica_availability",
    }
}

async fn resource_scope<T: RbacStore>(
    db: &T,
    resource_type: ResourceType,
    resource_id: uuid::Uuid,
    org_id: Option<uuid::Uuid>,
) -> Result<ScopeRef, SendableError> {
    if let Some(ownership) = db
        .fetch_resource_ownership(resource_type, resource_id)
        .await?
    {
        return Ok(ownership.owner);
    }
    Ok(org_id
        .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
        .unwrap_or(ScopeRef::PLATFORM))
}

async fn command_scope<T: RuntimeStore + RbacStore + OrchestrationStore>(
    db: &T,
    command: &WsIngressCommand,
) -> Result<ScopeRef, SendableError> {
    let workflow_run_id = match command {
        WsIngressCommand::SettleEffect { result, .. } => Some(result.workflow_run_id),
        WsIngressCommand::TimerInterrupt { timer, .. } => Some(timer.workflow_run_id),
        WsIngressCommand::Control {
            workflow_run_id, ..
        } => Some(*workflow_run_id),
        _ => None,
    };
    if let Some(run_id) = workflow_run_id
        && let Some(run) = db.fetch_workflow_run(run_id).await?
    {
        return resource_scope(db, ResourceType::Workflow, run.workflow_id, None).await;
    }
    if let WsIngressCommand::OrchestrationIntent { wake, .. } = command
        && let Some(binding) = db.fetch_orchestration_binding(wake.binding_id).await?
    {
        return resource_scope(
            db,
            ResourceType::Pipeline,
            binding.pipeline_id,
            binding.org_id,
        )
        .await;
    }
    Ok(ScopeRef::PLATFORM)
}

async fn inspect_or_apply<
    T: RuntimeStore + ReplicaStore + WorkflowVmStore + DeliveryStore + RbacStore + OrchestrationStore,
>(
    db: Arc<T>,
    broker: &dyn Broker,
    orchestration_nudge: &Notify,
    delivery: &IngressDelivery,
) -> Result<(), SendableError> {
    let scope = command_scope(db.as_ref(), &delivery.command).await?;
    let Some(session) = db.fetch_broker_ingress_session(scope).await? else {
        return apply_command(db, broker, orchestration_nudge, &delivery.command).await;
    };
    if session.mode == BrokerIngressSessionMode::Off {
        return apply_command(db, broker, orchestration_nudge, &delivery.command).await;
    }
    let hold = session.mode == BrokerIngressSessionMode::HoldOrchestrationNudges
        && matches!(
            delivery.command,
            WsIngressCommand::OrchestrationIntent { .. }
        );
    let command = serde_json::to_value(&delivery.command)
        .map(Value::from)
        .map_err(|error| Box::new(error) as SendableError)?;
    match db
        .capture_broker_ingress(BrokerIngressCaptureRequest {
            scope,
            delivery_id: delivery.delivery_id,
            dedupe_key: delivery.dedupe_key.clone(),
            command_kind: command_kind(&delivery.command).into(),
            command,
            hold,
            received_at: chrono::Utc::now(),
            capacity: INGRESS_CONTROL_QUEUE_CAPACITY,
        })
        .await?
    {
        BrokerIngressCapture::Full => Err(Box::new(std::io::Error::other(
            "broker ingress inspector queue is full",
        ))),
        BrokerIngressCapture::Held(_) => Ok(()),
        BrokerIngressCapture::Duplicate(record) if record.state != IngressControlState::Failed => {
            Ok(())
        }
        BrokerIngressCapture::Observed(record) | BrokerIngressCapture::Duplicate(record) => {
            match apply_command(db.clone(), broker, orchestration_nudge, &delivery.command).await {
                Ok(()) => {
                    db.finish_broker_ingress_record(
                        record.id,
                        IngressControlState::Applied,
                        None,
                        chrono::Utc::now(),
                    )
                    .await?;
                    Ok(())
                }
                Err(error) => {
                    db.finish_broker_ingress_record(
                        record.id,
                        IngressControlState::Failed,
                        Some(error.to_string()),
                        chrono::Utc::now(),
                    )
                    .await?;
                    Err(error)
                }
            }
        }
    }
}

async fn apply_command<T: RuntimeStore + ReplicaStore + WorkflowVmStore>(
    db: Arc<T>,
    broker: &dyn Broker,
    orchestration_nudge: &Notify,
    command: &WsIngressCommand,
) -> Result<(), SendableError> {
    match command {
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
