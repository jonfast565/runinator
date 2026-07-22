use std::{collections::HashMap, sync::Arc, time::Duration};

use runinator_broker::Broker;
use runinator_comm::{ControlCommand, ControlKind, WsIngressCommand};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::error_code_or_unknown;
use runinator_models::workflows::WorkflowStatus;
use tokio::sync::Notify;
use tracing::{Instrument, error, info, warn};
use uuid::Uuid;

use crate::{
    events::{AppEventKind, EventSender, emit, emit_pipeline_run, emit_workflow_run},
    repository, stability,
};

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
const READY_NODE_REAP_INTERVAL: Duration = Duration::from_secs(30);
const READY_NODE_REAP_LIMIT: i64 = 1000;

/// periodically announce pending ready nodes on the wake channel and re-announce any that were lost
/// (the durable backstop). the broker dedupes wakes already in flight. `wake_nudge` interrupts the
/// poll sleep when create/drive/result paths enqueue new ready work so queue→running is not gated
/// on [`WAKE_PUBLISH_INTERVAL`].
pub async fn run_wake_publisher<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    wake_nudge: Arc<Notify>,
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
            _ = wake_nudge.notified() => {}
            _ = tokio::time::sleep(WAKE_PUBLISH_INTERVAL) => {}
        }
    }
}

/// periodically mark replicas offline once they have gone quiet past the inactivity window, then
/// hard-delete rows that have stayed quiet far longer so offline replicas do not pile up forever.
/// the reducer-facing views derive stale state per fetch; this loop is the durable cleanup that
/// retires replicas that never sent an offline notice (e.g. crashed or evicted pods).
pub async fn run_replica_reaper<T: DatabaseImpl>(db: Arc<T>, shutdown: Arc<Notify>) {
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

/// safety backstop for ready-node bookkeeping: periodically settle any uncompleted ready nodes whose
/// run is already terminal. the reducer settles these inline on the terminal transition, so this
/// normally finds nothing; it exists for the instability case where that path did not run to
/// completion (a ws/broker/db crash mid-transition), preventing orphaned rows from being rescanned
/// by the wake publisher forever and bloating the ready table. batched so a large post-outage
/// backlog drains over several ticks rather than in one long-held lock.
pub async fn run_ready_node_reaper<T: DatabaseImpl>(db: Arc<T>, shutdown: Arc<Notify>) {
    info!("ready node reaper started");
    loop {
        match repository::settle_terminal_run_ready_nodes(db.as_ref(), READY_NODE_REAP_LIMIT).await
        {
            Ok(count) if count > 0 => {
                info!(count, "settled orphaned ready node(s) for terminal run(s)")
            }
            Ok(_) => {}
            Err(err) => error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "ready node reaper iteration failed: {}", err
            ),
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("ready node reaper shutting down");
                return;
            }
            _ = tokio::time::sleep(READY_NODE_REAP_INTERVAL) => {}
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

pub async fn run_usage_sampler<T: DatabaseImpl>(db: Arc<T>, shutdown: Arc<Notify>) {
    info!("usage sampler started");
    loop {
        match db.list_all_resource_groups().await {
            Ok(groups) => {
                // bucket the timestamp to the sampling-interval boundary so every instance sampling
                // the same window produces the same (org, backend, kind, sampled_at) key; the insert
                // is an idempotent DO-NOTHING upsert, so N-up sampling converges to one row per
                // window instead of over-counting node-hours by the instance count.
                let now = bucket_to_interval(chrono::Utc::now(), USAGE_SAMPLE_INTERVAL);
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
pub async fn run_trigger_loop<T: DatabaseImpl>(
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
                    // ready nodes were just enqueued for each fired run — do not wait for the wake
                    // publisher poll interval before announcing them.
                    events.nudge_wake_publisher();
                }
            }
            Err(err) => error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "trigger firing iteration failed: {}", err
            ),
        }

        // fire due cron pipeline triggers and start each created pipeline run's entry members.
        match repository::claim_due_pipeline_trigger_firings(
            db.as_ref(),
            instance_id.clone(),
            CLAIM_LIMIT,
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
                    events.nudge_wake_publisher();
                }
            }
            Err(err) => error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "pipeline trigger firing iteration failed: {}", err
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
/// `action_nudge` interrupts the poll sleep when a drive (or other path) enqueues outbox rows so
/// workers are not gated on [`ACTION_DISPATCH_INTERVAL`].
pub async fn run_action_dispatch_publisher<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance_id: String,
    action_nudge: Arc<Notify>,
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
            _ = action_nudge.notified() => {}
            _ = tokio::time::sleep(ACTION_DISPATCH_INTERVAL) => {}
        }
    }
}

/// consume the ingress channel: drive requests (from wakers) run the reducer, control requests
/// (from workers) pause/resume/cancel a run. the web service is the sole consumer.
pub async fn run_ingress_consumer<T: DatabaseImpl>(
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
                let org_id = repository::org_id_for_workflow_run(db, run_id).await;
                emit_workflow_run(events, run_id, org_id);
                // pipeline settle and member progression happen inside the drive with no other emit
                // site — fan a pipeline-run event so the UI does not fall back to the 30s poll.
                if let Ok(Some(run)) = db.fetch_workflow_run(run_id).await {
                    if let Some(pipeline_run_id) = run.pipeline_run_id {
                        let pipeline_org =
                            repository::org_id_for_pipeline_run(db, pipeline_run_id).await;
                        emit_pipeline_run(events, pipeline_run_id, pipeline_org);
                    }
                }
                // drive may have enqueued the next ready node(s) and/or action-dispatch outbox rows.
                events.nudge_wake_publisher();
                events.nudge_action_dispatch_publisher();
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
            let org_id = repository::org_id_for_workflow_run(db, *workflow_run_id).await;
            emit_workflow_run(events, *workflow_run_id, org_id);
            events.nudge_wake_publisher();
            events.nudge_action_dispatch_publisher();
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
