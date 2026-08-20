use std::{collections::HashMap, sync::Arc, time::Duration};

use runinator_broker_core::Broker;
use runinator_comm::{ControlCommand, ControlKind, WsIngressCommand};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::error_code_or_unknown;
use runinator_models::replicas::{ReplicaKind, ReplicaStatus};
use runinator_models::workflows::WorkflowStatus;
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    task::JoinSet,
};
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
const AGENT_DIRECTIVE_INTERVAL: Duration = Duration::from_secs(1);
const CLAIM_LIMIT: i64 = 100;
const ACTION_DISPATCH_LEASE_SECONDS: i64 = 60;
const MAX_INGRESS_ATTEMPTS: u32 = 3;
const INGRESS_RETRY_BACKOFF: Duration = Duration::from_millis(250);
const REPLICA_REAP_INTERVAL: Duration = Duration::from_secs(60);
const USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(300);
const READY_NODE_REAP_INTERVAL: Duration = Duration::from_secs(30);
const READY_NODE_REAP_LIMIT: i64 = 1000;
const OPERATIONAL_METRICS_INTERVAL: Duration = Duration::from_secs(15);

fn queue_age(
    now: chrono::DateTime<chrono::Utc>,
    oldest: Option<chrono::DateTime<chrono::Utc>>,
) -> u64 {
    oldest
        .map(|value| (now - value).num_seconds().max(0) as u64)
        .unwrap_or(0)
}

/// Periodically samples durable operational state so an idle deployment still has useful gauges.
/// This deliberately queries only aggregate queue/fleet state and never emits record identities.
pub async fn run_operational_metrics_sampler<T: DatabaseImpl>(db: Arc<T>, shutdown: Arc<Notify>) {
    info!("operational metrics sampler started");
    loop {
        let started = std::time::Instant::now();
        let now = chrono::Utc::now();
        let mut succeeded = true;

        match db.ready_node_queue_snapshots(now).await {
            Ok((due, future)) => {
                stability::queue_snapshot(
                    "ready_due",
                    due.depth,
                    due.claimed,
                    queue_age(now, due.oldest_enqueued_at),
                );
                stability::queue_snapshot(
                    "ready_future",
                    future.depth,
                    future.claimed,
                    queue_age(now, future.oldest_enqueued_at),
                );
            }
            Err(err) => {
                succeeded = false;
                stability::queue_failure("ready", "snapshot");
                warn!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "ready queue metrics snapshot failed: {err}"
                );
            }
        }
        match db.action_dispatch_queue_snapshot(now).await {
            Ok(snapshot) => stability::queue_snapshot(
                "action_dispatch",
                snapshot.depth,
                snapshot.claimed,
                queue_age(now, snapshot.oldest_enqueued_at),
            ),
            Err(err) => {
                succeeded = false;
                stability::queue_failure("action_dispatch", "snapshot");
                warn!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "action dispatch metrics snapshot failed: {err}"
                );
            }
        }
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

        let stale_before = now - chrono::Duration::seconds(repository::REPLICA_STALE_SECONDS);
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
            _ = tokio::time::sleep(OPERATIONAL_METRICS_INTERVAL) => {}
        }
    }
}

/// periodically announce pending ready nodes for drive. due nodes are driven directly on ingress;
/// future-dated nodes are published on the wake channel for the waker. `wake_nudge` interrupts the
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
        let started = std::time::Instant::now();
        let succeeded = if let Err(err) =
            repository::publish_pending_wakes(db.as_ref(), broker.as_ref(), CLAIM_LIMIT).await
        {
            stability::queue_failure("ready", "publish");
            error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "wake publisher iteration failed: {}", err
            );
            false
        } else {
            true
        };
        stability::loop_iteration("wake_publisher", succeeded, started.elapsed());
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
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match repository::reap_inactive_replicas(db.as_ref()).await {
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
        match repository::delete_expired_replicas(db.as_ref()).await {
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
        match repository::prune_replica_samples(db.as_ref()).await {
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
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match repository::settle_terminal_run_ready_nodes(db.as_ref(), READY_NODE_REAP_LIMIT).await
        {
            Ok(count) if count > 0 => {
                stability::cleanup("ready_node_reap", true, count);
                info!(count, "settled orphaned ready node(s) for terminal run(s)")
            }
            Ok(_) => {}
            Err(err) => {
                succeeded = false;
                stability::cleanup("ready_node_reap", false, 1);
                error!(
                    error_code = error_code_or_unknown(err.as_ref()),
                    "ready node reaper iteration failed: {}", err
                )
            }
        }
        stability::loop_iteration("ready_node_reaper", succeeded, started.elapsed());
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
        let started = std::time::Instant::now();
        let mut succeeded = true;
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
            _ = tokio::time::sleep(USAGE_SAMPLE_INTERVAL) => {}
        }
    }
}

/// periodically turn due workflow triggers into runs (formerly a waker loop, now in-process).
pub async fn run_trigger_loop<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    instance_id: String,
    shutdown: Arc<Notify>,
) {
    info!("trigger firing loop started");
    loop {
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match repository::claim_due_workflow_trigger_firings(
            db.as_ref(),
            instance_id.clone(),
            CLAIM_LIMIT,
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
                // a `cancel_previous` policy already settled the superseded runs durably; the
                // workers holding their in-flight actions still have to be told.
                for run_id in &batch.canceled_run_ids {
                    if let Err(err) = repository::release_run_mutexes(db.as_ref(), *run_id).await {
                        error!(run_id = %run_id, "failed to release canceled run mutexes: {err}");
                    }
                    repository::publish_run_cancel_commands(db.as_ref(), broker.as_ref(), *run_id)
                        .await;
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
                    // ready nodes were just enqueued for each fired run — do not wait for the wake
                    // publisher poll interval before announcing them.
                    events.nudge_wake_publisher();
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
        let started = std::time::Instant::now();
        let succeeded = if let Err(err) = repository::publish_pending_action_dispatches(
            db.as_ref(),
            broker.as_ref(),
            &instance_id,
            ACTION_DISPATCH_LEASE_SECONDS,
            CLAIM_LIMIT,
        )
        .await
        {
            stability::queue_failure("action_dispatch", "publish");
            error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "action dispatch publisher iteration failed: {}", err
            );
            false
        } else {
            true
        };
        stability::loop_iteration("action_dispatch", succeeded, started.elapsed());
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

/// drain the durable replica-directive outbox, with periodic redelivery as a reconnect backstop.
pub async fn run_agent_directive_publisher<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance_id: String,
    agent_nudge: Arc<Notify>,
    shutdown: Arc<Notify>,
) {
    info!("agent directive publisher started");
    loop {
        let started = std::time::Instant::now();
        let succeeded = if let Err(err) = repository::publish_due_agent_directives(
            db.as_ref(),
            broker.as_ref(),
            &instance_id,
            CLAIM_LIMIT,
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
            _ = tokio::time::sleep(AGENT_DIRECTIVE_INTERVAL) => {}
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
    max_concurrent_ingress: usize,
    shutdown: Arc<Notify>,
) {
    let max_concurrent_ingress = max_concurrent_ingress.max(1);
    info!(max_concurrent_ingress, "ingress consumer started");
    let attempts = Arc::new(Mutex::new(HashMap::<String, u32>::new()));
    let permits = Arc::new(Semaphore::new(max_concurrent_ingress));
    let mut deliveries = JoinSet::new();
    let mut health_tick = tokio::time::interval(std::time::Duration::from_secs(15));
    health_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        // Take capacity before receiving so a slow reducer never lets this consumer claim an
        // unbounded number of broker deliveries. The permit lives in the delivery task until its
        // ack/nack/dead-letter path has completed.
        let permit = tokio::select! {
            _ = shutdown.notified() => break,
            Some(result) = deliveries.join_next(), if !deliveries.is_empty() => {
                if let Err(err) = result {
                    error!("ingress delivery task join error: {err}");
                }
                continue;
            }
            _ = health_tick.tick() => {
                stability::loop_iteration("ingress", true, std::time::Duration::ZERO);
                continue;
            }
            permit = permits.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(err) => {
                    error!("ingress concurrency semaphore closed: {err}");
                    break;
                }
            },
        };
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                drop(permit);
                break;
            }
            received = broker.receive_ingress(INGRESS_CONSUMER_ID) => {
                match received {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        drop(permit);
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to receive ingress message: {}", err
                        );
                        // back off so an unreachable broker does not spin this loop hot.
                        tokio::select! {
                            _ = shutdown.notified() => {
                                break;
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
            WsIngressCommand::AgentDirectiveResult { result } => {
                tracing::info_span!("ingress_agent_directive_result", directive_id = %result.directive_id)
            }
        };
        let db = db.clone();
        let broker = broker.clone();
        let events = events.clone();
        let instance_id = instance_id.clone();
        let attempts = attempts.clone();
        deliveries.spawn(async move {
            let _permit = permit;
            process_ingress_delivery(db, broker, events, instance_id, attempts, delivery)
                .instrument(span)
                .await;
        });
    }

    info!(
        in_flight = deliveries.len(),
        "ingress consumer draining accepted deliveries"
    );
    while let Some(result) = deliveries.join_next().await {
        if let Err(err) = result {
            error!("ingress delivery task join error while draining: {err}");
        }
    }
    info!("ingress consumer shut down");
}

async fn process_ingress_delivery<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    instance_id: String,
    attempts: Arc<Mutex<HashMap<String, u32>>>,
    delivery: runinator_broker_core::IngressDelivery,
) {
    let started = std::time::Instant::now();
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
            let retry_depth = {
                let mut guard = attempts.lock().await;
                guard.remove(&key);
                guard.len() as u64
            };
            if let Err(err) = broker
                .ack_ingress(INGRESS_CONSUMER_ID, delivery.delivery_id)
                .await
            {
                error!(
                    error_code = error_code_or_unknown(&err),
                    "failed to ack ingress message: {}", err
                );
            }
            stability::queue_snapshot("ingress_retry", retry_depth, 0, 0);
            stability::loop_iteration("ingress", true, started.elapsed());
        }
        Err(err) => {
            let (count, retry_depth) = {
                let mut guard = attempts.lock().await;
                let entry = guard.entry(key.clone()).or_insert(0);
                *entry += 1;
                (*entry, guard.len() as u64)
            };
            error!(
                attempt = count,
                error_code = error_code_or_unknown(err.as_ref()),
                "failed to apply ingress message: {}",
                err
            );
            if count >= MAX_INGRESS_ATTEMPTS {
                stability::ingress_dead_lettered();
                let retry_depth = {
                    let mut guard = attempts.lock().await;
                    guard.remove(&key);
                    guard.len() as u64
                };
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
                stability::queue_snapshot("ingress_retry", retry_depth, 0, 0);
                stability::loop_iteration("ingress", false, started.elapsed());
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
            stability::queue_snapshot("ingress_retry", retry_depth, 0, 0);
            stability::loop_iteration("ingress", false, started.elapsed());
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
                    // the drive is where a run reaches a terminal state, so it is also where the
                    // failure-alerting policies are evaluated. best-effort: alerting never fails the
                    // drive that produced the run it is reporting on.
                    if run.status.is_terminal() {
                        crate::notifications::on_run_terminal(db, events, run_id).await;
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
        WsIngressCommand::AgentDirectiveResult { result } => {
            repository::complete_agent_directive(db, result.clone()).await?;
            emit(
                events,
                crate::events::AppEvent::global(AppEventKind::ReplicasChanged),
            );
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
