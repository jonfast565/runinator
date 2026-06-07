use std::{collections::HashMap, sync::Arc, time::Duration};

use log::{error, info, warn};
use runinator_broker::Broker;
use runinator_comm::{ControlKind, WsIngressCommand};
use runinator_database::interfaces::DatabaseImpl;
use tokio::sync::{Notify, broadcast};
use uuid::Uuid;

use crate::{
    events::{AppEvent, EventSender, emit, emit_workflow_run},
    repository,
};

const EVENT_CONSUMER_RETRY_BACKOFF: Duration = Duration::from_millis(250);

const INGRESS_CONSUMER_ID: &str = "runinator-ws-ingress";
const WAKE_PUBLISH_INTERVAL: Duration = Duration::from_millis(1000);
const TRIGGER_INTERVAL: Duration = Duration::from_millis(1000);
const ACTION_DISPATCH_INTERVAL: Duration = Duration::from_millis(500);
const CLAIM_LIMIT: i64 = 100;
const ACTION_DISPATCH_LEASE_SECONDS: i64 = 60;
const MAX_INGRESS_ATTEMPTS: u32 = 3;
const INGRESS_RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// periodically announce pending ready nodes on the wake channel and re-announce any that were lost
/// (the durable backstop). the broker dedupes wakes already in flight.
pub(crate) async fn run_wake_publisher<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    shutdown: Arc<Notify>,
) {
    info!("Wake publisher started");
    loop {
        if let Err(err) =
            repository::publish_pending_wakes(db.as_ref(), broker.as_ref(), CLAIM_LIMIT).await
        {
            error!("Wake publisher iteration failed: {}", err);
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("Wake publisher shutting down");
                return;
            }
            _ = tokio::time::sleep(WAKE_PUBLISH_INTERVAL) => {}
        }
    }
}

/// periodically turn due workflow triggers into runs (formerly a waker loop, now in-process).
pub(crate) async fn run_trigger_loop<T: DatabaseImpl>(
    db: Arc<T>,
    events: EventSender,
    instance_id: String,
    shutdown: Arc<Notify>,
) {
    info!("Trigger firing loop started");
    loop {
        match repository::claim_due_workflow_trigger_firings(
            db.as_ref(),
            instance_id.clone(),
            CLAIM_LIMIT,
        )
        .await
        {
            Ok(runs) => {
                for run in &runs {
                    emit_workflow_run(&events, run.id);
                }
                if !runs.is_empty() {
                    emit(&events, AppEvent::WorkflowRunActivity);
                }
            }
            Err(err) => error!("Trigger firing iteration failed: {}", err),
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("Trigger firing loop shutting down");
                return;
            }
            _ = tokio::time::sleep(TRIGGER_INTERVAL) => {}
        }
    }
}

/// periodically drain durable action-dispatch intents and publish them to the broker action channel.
pub(crate) async fn run_action_dispatch_publisher<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance_id: String,
    shutdown: Arc<Notify>,
) {
    info!("Action dispatch publisher started");
    loop {
        if let Err(err) = repository::publish_pending_action_dispatches(
            db.as_ref(),
            broker.as_ref(),
            &instance_id,
            ACTION_DISPATCH_LEASE_SECONDS,
            CLAIM_LIMIT,
        )
        .await
        {
            error!("Action dispatch publisher iteration failed: {}", err);
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("Action dispatch publisher shutting down");
                return;
            }
            _ = tokio::time::sleep(ACTION_DISPATCH_INTERVAL) => {}
        }
    }
}

/// consume the ingress channel: drive requests (from wakers) run the reducer, control requests
/// (from workers) pause/resume/cancel a run. the web service is the sole consumer.
pub(crate) async fn run_ingress_consumer<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    instance_id: String,
    shutdown: Arc<Notify>,
) {
    info!("Ingress consumer started");
    let mut attempts = HashMap::<String, u32>::new();
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                info!("Ingress consumer shutting down");
                return;
            }
            received = broker.receive_ingress(INGRESS_CONSUMER_ID) => {
                match received {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        error!("Failed to receive ingress message: {}", err);
                        continue;
                    }
                }
            }
        };

        let key = delivery.dedupe_key.clone();
        match apply_ingress(
            db.as_ref(),
            broker.as_ref(),
            &events,
            &instance_id,
            &delivery.command,
        )
        .await
        {
            Ok(()) => {
                attempts.remove(&key);
                if let Err(err) = broker
                    .ack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    error!("Failed to ack ingress message: {}", err);
                }
            }
            Err(err) => {
                let count = {
                    let entry = attempts.entry(key.clone()).or_insert(0);
                    *entry += 1;
                    *entry
                };
                error!(
                    "Failed to apply ingress message on attempt {}: {}",
                    count, err
                );
                if count >= MAX_INGRESS_ATTEMPTS {
                    attempts.remove(&key);
                    warn!("Dead-lettering ingress message after {} attempt(s)", count);
                    if let Err(err) = broker
                        .ack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        error!("Failed to ack dead-lettered ingress message: {}", err);
                    }
                    continue;
                }
                tokio::time::sleep(INGRESS_RETRY_BACKOFF).await;
                if let Err(err) = broker
                    .nack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    error!("Failed to nack ingress message: {}", err);
                }
            }
        }
    }
}

async fn apply_ingress<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    events: &EventSender,
    instance_id: &str,
    command: &WsIngressCommand,
) -> Result<(), runinator_models::errors::SendableError> {
    match command {
        WsIngressCommand::Drive { ready_node_id, .. } => {
            if let Some(run_id) =
                repository::drive_ready_node(db, *ready_node_id, instance_id.to_string()).await?
            {
                emit_workflow_run(events, run_id);
            }
            Ok(())
        }
        WsIngressCommand::Control {
            workflow_run_id,
            kind,
        } => {
            match kind {
                ControlKind::Cancel => {
                    repository::cancel_workflow_run(db, broker, *workflow_run_id).await?;
                }
                ControlKind::Pause => {
                    repository::pause_workflow_run(db, *workflow_run_id).await?;
                }
                ControlKind::Resume => {
                    repository::resume_workflow_run(db, *workflow_run_id).await?;
                }
            }
            emit_workflow_run(events, *workflow_run_id);
            Ok(())
        }
    }
}

/// consume the broker fan-out events channel and re-broadcast every event to this replica's local
/// WebSocket clients. each replica subscribes with its own per-replica `instance_id`, so every
/// replica receives every UI event regardless of which replica emitted it.
pub(crate) async fn run_event_consumer(
    broker: Arc<dyn Broker>,
    local: broadcast::Sender<AppEvent>,
    instance_id: String,
    shutdown: Arc<Notify>,
) {
    info!("Event consumer started");
    loop {
        let received = tokio::select! {
            _ = shutdown.notified() => {
                info!("Event consumer shutting down");
                return;
            }
            received = broker.receive_event(&instance_id) => received,
        };
        match received {
            // a send error just means no WebSocket clients are connected right now; events are
            // best-effort, so drop it.
            Ok(delivery) => {
                let _ = local.send(delivery.event);
            }
            Err(err) => {
                error!("Failed to receive UI event: {}", err);
                tokio::select! {
                    _ = shutdown.notified() => return,
                    _ = tokio::time::sleep(EVENT_CONSUMER_RETRY_BACKOFF) => {}
                }
            }
        }
    }
}

/// a stable per-process identifier used when claiming database rows.
pub(crate) fn instance_id() -> String {
    format!("runinator-ws-{}", Uuid::new_v4())
}
