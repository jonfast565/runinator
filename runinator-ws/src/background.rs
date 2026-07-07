use std::{collections::HashMap, sync::Arc, time::Duration};

use runinator_broker::Broker;
use runinator_comm::{ControlCommand, ControlKind, WsIngressCommand};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::error_code_or_unknown;
use runinator_models::workflows::WorkflowStatus;
use tokio::sync::{Notify, broadcast};
use tracing::{Instrument, error, info, warn};
use uuid::Uuid;

use crate::{
    events::{AppEvent, EventSender, emit, emit_workflow_run},
    repository, stability,
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
const REPLICA_REAP_INTERVAL: Duration = Duration::from_secs(60);
const USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(300);

/// periodically announce pending ready nodes on the wake channel and re-announce any that were lost
/// (the durable backstop). the broker dedupes wakes already in flight.
pub(crate) async fn run_wake_publisher<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    shutdown: Arc<Notify>,
) {
    info!("wake publisher started");
    loop {
        if let Err(err) =
            repository::publish_pending_wakes(db.as_ref(), broker.as_ref(), CLAIM_LIMIT).await
        {
            error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "wake publisher iteration failed: {}", err
            );
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("wake publisher shutting down");
                return;
            }
            _ = tokio::time::sleep(WAKE_PUBLISH_INTERVAL) => {}
        }
    }
}

/// periodically mark replicas offline once they have gone quiet past the inactivity window, then
/// hard-delete rows that have stayed quiet far longer so offline replicas do not pile up forever.
/// the reducer-facing views derive stale state per fetch; this loop is the durable cleanup that
/// retires replicas that never sent an offline notice (e.g. crashed or evicted pods).
pub(crate) async fn run_replica_reaper<T: DatabaseImpl>(db: Arc<T>, shutdown: Arc<Notify>) {
    info!("replica reaper started");
    loop {
        match repository::reap_inactive_replicas(db.as_ref()).await {
            Ok(count) if count > 0 => info!(count, "reaped inactive replica(s) to offline"),
            Ok(_) => {}
            Err(err) => error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "replica reaper iteration failed: {}", err
            ),
        }
        match repository::delete_expired_replicas(db.as_ref()).await {
            Ok(count) if count > 0 => info!(count, "purged long-stale replica(s)"),
            Ok(_) => {}
            Err(err) => error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "replica purge iteration failed: {}", err
            ),
        }
        match repository::prune_replica_samples(db.as_ref()).await {
            Ok(count) if count > 0 => info!(count, "pruned expired replica sample(s)"),
            Ok(_) => {}
            Err(err) => error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "replica sample prune iteration failed: {}", err
            ),
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("replica reaper shutting down");
                return;
            }
            _ = tokio::time::sleep(REPLICA_REAP_INTERVAL) => {}
        }
    }
}

/// periodically record each org's dedicated node allocation into the usage ledger so per-org
/// node-hours (and cost) can be integrated over time. sampling the recorded allocations keeps
/// accounting exact and provisioner-independent; a missed sample only reduces temporal resolution.
pub(crate) async fn run_usage_sampler<T: DatabaseImpl>(db: Arc<T>, shutdown: Arc<Notify>) {
    info!("usage sampler started");
    loop {
        match db.list_all_resource_groups().await {
            Ok(groups) => {
                let now = chrono::Utc::now();
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
                        warn!(
                            org_id = %org_id,
                            error_code = error_code_or_unknown(err.as_ref()),
                            "usage sample insert failed: {}", err
                        );
                    }
                }
            }
            Err(err) => error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "usage sampler iteration failed: {}", err
            ),
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("usage sampler shutting down");
                return;
            }
            _ = tokio::time::sleep(USAGE_SAMPLE_INTERVAL) => {}
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
    info!("trigger firing loop started");
    loop {
        match repository::claim_due_workflow_trigger_firings(
            db.as_ref(),
            instance_id.clone(),
            CLAIM_LIMIT,
        )
        .await
        {
            Ok(runs) => {
                stability::triggers_fired(runs.len() as u64);
                if !runs.is_empty() {
                    info!(count = runs.len(), "fired due workflow trigger(s)");
                }
                for run in &runs {
                    emit_workflow_run(&events, run.id);
                }
                if !runs.is_empty() {
                    emit(&events, AppEvent::WorkflowRunActivity);
                }
            }
            Err(err) => error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "trigger firing iteration failed: {}", err
            ),
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("trigger firing loop shutting down");
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
    info!("action dispatch publisher started");
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
            error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "action dispatch publisher iteration failed: {}", err
            );
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("action dispatch publisher shutting down");
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
    info!("ingress consumer started");
    let mut attempts = HashMap::<String, u32>::new();
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                info!("ingress consumer shutting down");
                return;
            }
            received = broker.receive_ingress(INGRESS_CONSUMER_ID) => {
                match received {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to receive ingress message: {}", err
                        );
                        // back off so an unreachable broker does not spin this loop hot.
                        tokio::select! {
                            _ = shutdown.notified() => {
                                info!("ingress consumer shutting down");
                                return;
                            }
                            _ = tokio::time::sleep(INGRESS_RETRY_BACKOFF) => {}
                        }
                        continue;
                    }
                }
            }
        };

        // correlate this delivery's logs with the run/node it targets. `Drive` carries the reducer's
        // identity; `Control` only carries the run, since it is not node-scoped.
        let span = match &delivery.command {
            WsIngressCommand::Drive {
                workflow_run_id,
                node_id,
                trace_id,
                ..
            } => tracing::info_span!(
                "ingress_drive",
                trace_id = %trace_id,
                run_id = %workflow_run_id,
                node_id = %node_id
            ),
            WsIngressCommand::Control {
                workflow_run_id, ..
            } => tracing::info_span!("ingress_control", run_id = %workflow_run_id),
        };

        async {
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
                    stability::ingress_applied();
                    attempts.remove(&key);
                    if let Err(err) = broker
                        .ack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to ack ingress message: {}", err
                        );
                    }
                }
                Err(err) => {
                    let count = {
                        let entry = attempts.entry(key.clone()).or_insert(0);
                        *entry += 1;
                        *entry
                    };
                    error!(
                        attempt = count,
                        error_code = error_code_or_unknown(err.as_ref()),
                        "failed to apply ingress message: {}",
                        err
                    );
                    if count >= MAX_INGRESS_ATTEMPTS {
                        stability::ingress_dead_lettered();
                        attempts.remove(&key);
                        warn!(attempts = count, "dead-lettering ingress message");
                        crate::audit::persist_dead_letter(
                            db.as_ref(),
                            "ingress",
                            None,
                            Some(delivery.dedupe_key.clone()),
                            count,
                            &err.to_string(),
                            serde_json::to_value(&delivery.command).unwrap_or_default(),
                        )
                        .await;
                        if let Err(err) = broker
                            .ack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                            .await
                        {
                            error!(
                                error_code = error_code_or_unknown(&err),
                                "failed to ack dead-lettered ingress message: {}", err
                            );
                        }
                        return;
                    }
                    stability::ingress_retried();
                    tokio::time::sleep(INGRESS_RETRY_BACKOFF).await;
                    if let Err(err) = broker
                        .nack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to nack ingress message: {}", err
                        );
                    }
                }
            }
        }
        .instrument(span)
        .await;
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
            let started = std::time::Instant::now();
            let driven =
                repository::drive_ready_node(db, *ready_node_id, instance_id.to_string()).await?;
            stability::record_reducer_drive_ms(started.elapsed().as_secs_f64() * 1000.0);
            if let Some(run_id) = driven {
                signal_canceled_executing_node_runs(db, broker, run_id).await;
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

/// publish a node-run-targeted worker cancel for every node run the reducer has just marked
/// `Canceled` while a worker still holds its executor lease (e.g. a losing race branch). best-effort:
/// a missed signal at worst lets the loser run to completion, the pre-existing v1 behavior. idempotent
/// across drives because the worker clears its executor claim once the cancel lands.
async fn signal_canceled_executing_node_runs<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    workflow_run_id: Uuid,
) {
    let node_runs = match db.fetch_workflow_node_runs(workflow_run_id).await {
        Ok(node_runs) => node_runs,
        Err(err) => {
            warn!(
                run_id = %workflow_run_id,
                error_code = error_code_or_unknown(err.as_ref()),
                "failed to load node runs for cancel fan-out: {}",
                err
            );
            return;
        }
    };
    for run in node_runs {
        let Some(executor_replica_id) = run.current_executor_replica_id else {
            continue;
        };
        if run.status != WorkflowStatus::Canceled {
            continue;
        }
        // route the cancel to the replica holding the executor lease so it is not consumed (and
        // dropped) by a worker that never dispatched this action.
        let command = ControlCommand::for_node_run(workflow_run_id, run.id, ControlKind::Cancel)
            .targeting_replica(executor_replica_id);
        if let Err(err) = broker.publish_control(command).await {
            warn!(
                run_id = %workflow_run_id,
                node_run_id = %run.id,
                error_code = error_code_or_unknown(&err),
                "failed to publish cancel: {}",
                err
            );
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
    info!("event consumer started");
    loop {
        let received = tokio::select! {
            _ = shutdown.notified() => {
                info!("event consumer shutting down");
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
                error!(
                    error_code = error_code_or_unknown(&err),
                    "failed to receive UI event: {}", err
                );
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
