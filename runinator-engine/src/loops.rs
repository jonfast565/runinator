use std::{sync::Arc, time::Duration};

use runinator_broker_core::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::error_code_or_unknown;
use runinator_models::replicas::{ReplicaKind, ReplicaStatus};
use tokio::sync::Notify;
use tracing::{error, info, warn};

use crate::{
    events::{AppEventKind, EventSender, emit, emit_pipeline_run, emit_workflow_run},
    repository, stability,
};

const TRIGGER_INTERVAL: Duration = Duration::from_millis(1000);
const AGENT_DIRECTIVE_INTERVAL: Duration = Duration::from_secs(1);
const CLAIM_LIMIT: i64 = 100;
const ACTION_DISPATCH_LEASE_SECONDS: i64 = 60;
const REPLICA_REAP_INTERVAL: Duration = Duration::from_secs(60);
const USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(300);
const OPERATIONAL_METRICS_INTERVAL: Duration = Duration::from_secs(15);
const WORKFLOW_VM_DRIVE_INTERVAL: Duration = Duration::from_millis(250);
const WORKFLOW_EFFECT_DISPATCH_INTERVAL: Duration = Duration::from_millis(250);

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
pub async fn run_workflow_vm_driver<T: DatabaseImpl>(
    db: Arc<T>,
    instance: String,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM driver started");
    let host = runinator_runtime::WorkflowVmHost::new(db.as_ref());
    loop {
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match host.drive_runnable(instance.clone(), CLAIM_LIMIT).await {
            Ok(outcomes) => {
                for outcome in outcomes {
                    stability::vm_continuation_driven(match outcome {
                        runinator_runtime::WorkflowVmDriveOutcome::Yielded => "yielded",
                        runinator_runtime::WorkflowVmDriveOutcome::Forked => "forked",
                        runinator_runtime::WorkflowVmDriveOutcome::Joined => "joined",
                        runinator_runtime::WorkflowVmDriveOutcome::Completed { .. } => "completed",
                        runinator_runtime::WorkflowVmDriveOutcome::Failed { .. } => "failed",
                    });
                    let settled_run_id = match outcome {
                        runinator_runtime::WorkflowVmDriveOutcome::Completed { settled_run_id }
                        | runinator_runtime::WorkflowVmDriveOutcome::Failed { settled_run_id } => {
                            settled_run_id
                        }
                        _ => None,
                    };
                    if let Some(run_id) = settled_run_id {
                        if let Err(err) =
                            repository::advance_pipeline_from_vm_terminal(db.as_ref(), run_id).await
                        {
                            warn!(workflow_run_id = %run_id, error = %err, "VM pipeline advancement failed");
                        }
                        match db.fetch_workflow_run(run_id).await {
                            Ok(Some(run)) => {
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
                }
            }
            Err(err) => {
                succeeded = false;
                stability::vm_driver_failure();
                warn!(error = %err, "workflow VM drive failed");
            }
        }
        match db.fetch_unsettled_vm_pipeline_members(CLAIM_LIMIT).await {
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
        stability::record_vm_drive_duration_ms(started.elapsed().as_secs_f64() * 1000.0);
        stability::loop_iteration("workflow_vm_driver", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = tokio::time::sleep(WORKFLOW_VM_DRIVE_INTERVAL) => {}
        }
    }
}

/// Drain the VM effect outbox. The command was frozen in the same transaction as the suspended
/// continuation, so this publisher never re-reads graph or node-run state to rebuild a delivery.
pub async fn run_workflow_effect_dispatcher<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance: String,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM effect dispatcher started");
    loop {
        let now = chrono::Utc::now();
        match db
            .claim_pending_workflow_effect_dispatches(
                instance.clone(),
                now,
                now + chrono::Duration::seconds(ACTION_DISPATCH_LEASE_SECONDS),
                CLAIM_LIMIT,
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
        tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(WORKFLOW_EFFECT_DISPATCH_INTERVAL) => {} }
    }
}

/// Periodically samples durable operational state so an idle deployment still has useful gauges.
/// This deliberately queries only aggregate queue/fleet state and never emits record identities.
pub async fn run_operational_metrics_sampler<T: DatabaseImpl>(db: Arc<T>, shutdown: Arc<Notify>) {
    info!("operational metrics sampler started");
    loop {
        let started = std::time::Instant::now();
        let now = chrono::Utc::now();
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
