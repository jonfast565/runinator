use std::{sync::Arc, time::Duration};

use runinator_broker_core::{Broker, WakeMessage};
use runinator_comm::WakeCommand;
use runinator_models::errors::error_code_or_unknown;
use runinator_models::replicas::{ReplicaKind, ReplicaStatus};
use runinator_models::{
    orchestration::{IngressPromotion, IngressTargetKind},
    replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance},
    workflow_vm::{WorkflowEffectRequest, WorkflowEffectStatus},
    workspaces::WorkspaceAffinity,
};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, IngressStore, NotificationStore, OrgStore, ReplicaStore, ScheduleStore,
        WorkflowVmStore, WorkspaceStore,
    },
};
use tokio::sync::Notify;
use tracing::{error, info, warn};

use crate::{
    events::{AppEventKind, EventSender, emit, emit_pipeline_run, emit_workflow_run},
    repository,
    services::{ReplicaRegistry, WorkspaceOperations, WorkspaceRecovery},
    settings::ServerSettingsHandle,
    stability,
};

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
pub async fn run_workflow_vm_driver<
    T: RuntimeStore + WorkflowVmStore + IngressStore + DefinitionStore,
>(
    db: Arc<T>,
    instance: String,
    ready_nudge: Arc<Notify>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM driver started");
    let host = runinator_runtime::WorkflowVmHost::new(db.as_ref());
    loop {
        let started = std::time::Instant::now();
        let mut succeeded = true;
        let policy = settings.current();
        let claim_limit = policy.orchestration.claim_batch_size as i64;
        match host.drive_runnable(instance.clone(), claim_limit).await {
            Ok(outcomes) => {
                for outcome in outcomes {
                    stability::vm_continuation_driven(match outcome {
                        runinator_runtime::WorkflowVmDriveOutcome::Yielded => "yielded",
                        runinator_runtime::WorkflowVmDriveOutcome::Forked => "forked",
                        runinator_runtime::WorkflowVmDriveOutcome::Joined => "joined",
                        runinator_runtime::WorkflowVmDriveOutcome::Completed { .. } => "completed",
                        runinator_runtime::WorkflowVmDriveOutcome::Failed { .. } => "failed",
                        runinator_runtime::WorkflowVmDriveOutcome::Interrupted => "interrupted",
                        runinator_runtime::WorkflowVmDriveOutcome::InterruptResolved { .. } => {
                            "interrupt_resolved"
                        }
                    });
                    let settled_run_id = match outcome {
                        runinator_runtime::WorkflowVmDriveOutcome::Completed { settled_run_id }
                        | runinator_runtime::WorkflowVmDriveOutcome::Failed { settled_run_id }
                        | runinator_runtime::WorkflowVmDriveOutcome::InterruptResolved {
                            settled_run_id,
                        } => settled_run_id,
                        _ => None,
                    };
                    if let Some(run_id) = settled_run_id {
                        match db
                            .settle_and_promote_ingress_workflow_run(run_id, chrono::Utc::now())
                            .await
                        {
                            Ok(Some(promotion)) => {
                                start_ingress_promotion(db.as_ref(), promotion).await
                            }
                            Ok(None) => {}
                            Err(err) => {
                                warn!(workflow_run_id = %run_id, error = %err, "ingress workflow settlement failed")
                            }
                        }
                        if let Err(err) =
                            repository::advance_pipeline_from_vm_terminal(db.as_ref(), run_id).await
                        {
                            warn!(workflow_run_id = %run_id, error = %err, "VM pipeline advancement failed");
                        }
                        match db.fetch_workflow_run(run_id).await {
                            Ok(Some(run)) => {
                                if let Some(pipeline_run_id) = run.pipeline_run_id {
                                    match db.fetch_pipeline_run(pipeline_run_id).await {
                                        Ok(Some(pipeline_run))
                                            if pipeline_run.status.is_terminal() =>
                                        {
                                            match db
                                                .settle_and_promote_ingress_pipeline_run(
                                                    pipeline_run_id,
                                                    chrono::Utc::now(),
                                                )
                                                .await
                                            {
                                                Ok(Some(promotion)) => {
                                                    start_ingress_promotion(db.as_ref(), promotion)
                                                        .await
                                                }
                                                Ok(None) => {}
                                                Err(err) => {
                                                    warn!(pipeline_run_id = %pipeline_run_id, error = %err, "ingress pipeline settlement failed")
                                                }
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(err) => {
                                            warn!(pipeline_run_id = %pipeline_run_id, error = %err, "failed to load pipeline run for ingress settlement")
                                        }
                                    }
                                }
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
        match db.fetch_unsettled_vm_pipeline_members(claim_limit).await {
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
        // A startup failure releases its claim back to the FIFO head. Reconciliation retries one
        // such head each driver pass, including after process restart.
        match db.claim_queued_ingress_event(chrono::Utc::now()).await {
            Ok(Some(promotion)) => start_ingress_promotion(db.as_ref(), promotion).await,
            Ok(None) => {}
            Err(err) => warn!(error = %err, "failed to reconcile queued ingress event"),
        }
        stability::record_vm_drive_duration_ms(started.elapsed().as_secs_f64() * 1000.0);
        stability::loop_iteration("workflow_vm_driver", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = ready_nudge.notified() => {}
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.workflow_vm_poll_interval_ms)) => {}
        }
    }
}

async fn start_ingress_promotion<
    T: RuntimeStore + WorkflowVmStore + IngressStore + DefinitionStore,
>(
    db: &T,
    promotion: IngressPromotion,
) {
    let event_id = promotion.event.id;
    let admission_id = promotion.admission.id.expect("stored admission id");
    let result = match promotion.admission.target.kind {
        IngressTargetKind::Workflow => {
            let provenance = WorkflowRunProvenance {
                source_kind: Some(TriggerSourceKind::Api),
                actor_type: Some(TriggerActorType::System),
                actor_replica_id: None,
                actor_display_name: Some("ingress queue".into()),
                request_host: None,
                request_ip: None,
                metadata: runinator_models::json!({
                    "ingress_source": promotion.event.source,
                    "ingress_event_id": promotion.event.event_id,
                    "ingress_generation": promotion.admission.generation,
                }),
            };
            match repository::create_workflow_run(
                db,
                promotion.admission.target.id,
                promotion.event.payload.clone(),
                false,
                Some(format!("ingress:{}", promotion.event.event_id)),
                provenance,
            )
            .await
            {
                Ok(run) => db
                    .bind_ingress_workflow_run(admission_id, run.id, chrono::Utc::now())
                    .await
                    .and_then(|bound| {
                        if bound {
                            Ok(Some((Some(run.id), None)))
                        } else {
                            Err(Box::new(std::io::Error::other(
                                "promoted workflow admission bind lost",
                            )))
                        }
                    }),
                Err(err) => Err(err),
            }
        }
        IngressTargetKind::Pipeline => match repository::create_manual_pipeline_run(
            db,
            promotion.admission.target.id,
            promotion.event.payload.clone(),
            None,
            None,
            Some("ingress queue".into()),
        )
        .await
        {
            Ok(run) => db
                .bind_ingress_pipeline_run(admission_id, run.id, chrono::Utc::now())
                .await
                .and_then(|bound| {
                    if bound {
                        Ok(Some((None, Some(run.id))))
                    } else {
                        Err(Box::new(std::io::Error::other(
                            "promoted pipeline admission bind lost",
                        )))
                    }
                }),
            Err(err) => Err(err),
        },
    };
    match result {
        Ok(Some((workflow_run_id, pipeline_run_id))) => {
            if let Err(err) = db
                .bind_ingress_event_result(
                    event_id,
                    workflow_run_id,
                    pipeline_run_id,
                    chrono::Utc::now(),
                )
                .await
            {
                warn!(ingress_event_id = %event_id, error = %err, "failed to bind promoted ingress event result");
            }
        }
        Ok(None) => {}
        Err(err) => {
            warn!(ingress_event_id = %event_id, error = %err, "queued ingress startup failed; releasing FIFO claim");
            if let Err(release_err) = db
                .release_ingress_promotion(promotion.claim_token, chrono::Utc::now())
                .await
            {
                warn!(ingress_event_id = %event_id, error = %release_err, "failed to release queued ingress claim");
            }
        }
    }
}

/// Arm declared periodic interrupt timers through the broker-only waker.
///
/// The schedule itself is durable; re-publishing the same not-yet-due occurrence is harmless
/// because the wake key includes the run, timer declaration, and exact due instant. This keeps the
/// engine from sleeping on a run-local timer and lets any waker relay the due occurrence.
pub async fn run_timer_interrupt_scheduler<T: WorkflowVmStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("workflow timer-interrupt scheduler started");
    loop {
        let started = std::time::Instant::now();
        let now = chrono::Utc::now();
        let policy = settings.current();
        let mut succeeded = true;
        match db
            .fetch_workflow_timer_interrupts_before(
                now + chrono::Duration::milliseconds(
                    policy.orchestration.timer_arm_horizon_ms as i64,
                ),
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(timers) => {
                for timer in timers {
                    let wake = WakeCommand::timer_interrupt(
                        timer.due_at,
                        timer.workflow_run_id,
                        timer.timer_id.clone(),
                        timer.interval_seconds,
                        uuid::Uuid::now_v7(),
                    );
                    match broker
                        .publish_wake(WakeMessage {
                            dedupe_key: Some(wake.dedupe_key()),
                            command: wake,
                            enqueued_at: now,
                        })
                        .await
                    {
                        Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {}
                        Err(err) => {
                            succeeded = false;
                            warn!(
                                workflow_run_id = %timer.workflow_run_id,
                                timer_id = %timer.timer_id,
                                error = %err,
                                "failed to arm workflow timer interrupt"
                            );
                        }
                    }
                }
            }
            Err(err) => {
                succeeded = false;
                warn!(error = %err, "failed to load workflow timer interrupts to arm");
            }
        }
        stability::loop_iteration("timer_interrupt_scheduler", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.workflow_vm_poll_interval_ms)) => {}
        }
    }
}

/// Drain the VM effect outbox. The command was frozen in the same transaction as the suspended
/// continuation, so this publisher never re-reads graph or node-run state to rebuild a delivery.
pub async fn run_workflow_effect_dispatcher<T: WorkflowVmStore + WorkspaceStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance: String,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM effect dispatcher started");
    loop {
        let now = chrono::Utc::now();
        let policy = settings.current();
        match db
            .claim_pending_workflow_effect_dispatches(
                instance.clone(),
                now,
                now + chrono::Duration::seconds(
                    policy.orchestration.action_dispatch_lease_seconds as i64,
                ),
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(dispatches) => {
                for dispatch in dispatches {
                    match workspace_affinity_is_current(db.as_ref(), &dispatch.command.request)
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            let message = "workspace affinity is stale or no longer active";
                            if let Err(error) = db
                                .settle_workflow_effect(
                                    dispatch.command.effect_id,
                                    dispatch.command.attempt,
                                    WorkflowEffectStatus::Rejected,
                                    None,
                                    Some(message.into()),
                                    now,
                                )
                                .await
                            {
                                warn!(error = %error, dispatch_id = %dispatch.id, "failed to reject stale workspace effect");
                                let _ = db
                                    .mark_workflow_effect_dispatch_failed(
                                        dispatch.id,
                                        error.to_string(),
                                    )
                                    .await;
                                continue;
                            }
                            if let Err(error) = db
                                .mark_workflow_effect_dispatch_published(dispatch.id)
                                .await
                            {
                                warn!(error = %error, dispatch_id = %dispatch.id, "failed to acknowledge rejected workspace effect");
                            }
                            continue;
                        }
                        Err(error) => {
                            warn!(error = %error, dispatch_id = %dispatch.id, "failed to validate workspace affinity");
                            let _ = db
                                .mark_workflow_effect_dispatch_failed(
                                    dispatch.id,
                                    error.to_string(),
                                )
                                .await;
                            continue;
                        }
                    }
                    // kept for the deadline arming below, since publishing consumes the command.
                    let published_command = dispatch.command.clone();
                    match broker
                        .publish_effect(runinator_broker_core::EffectMessage {
                            dedupe_key: Some(dispatch.dedupe_key.clone()),
                            command: dispatch.command,
                            enqueued_at: now,
                        })
                        .await
                    {
                        Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {
                            // armed after publication, never before: the backstop must not be able
                            // to stop the work it protects.
                            crate::effect_deadline::arm_with_grace(
                                broker.as_ref(),
                                &published_command,
                                now,
                                policy.orchestration.action_deadline_grace_seconds as i64,
                            )
                            .await;
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
        tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.effect_dispatch_poll_interval_ms)) => {} }
    }
}

async fn workspace_affinity_is_current<T: WorkspaceStore>(
    db: &T,
    request: &WorkflowEffectRequest,
) -> Result<bool, runinator_models::errors::SendableError> {
    let WorkflowEffectRequest::Action {
        workspace_affinity: Some(value),
        ..
    } = request
    else {
        return Ok(true);
    };
    let affinity: WorkspaceAffinity =
        serde_json::from_value(value.clone().into()).map_err(|error| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid workspace affinity: {error}"),
            )) as runinator_models::errors::SendableError
        })?;
    let Some(workspace) = db.fetch_workspace(affinity.workspace_id).await? else {
        return Ok(false);
    };
    Ok(workspace_affinity_matches(&workspace, &affinity))
}

fn workspace_affinity_matches(
    workspace: &runinator_models::workspaces::WorkspaceLease,
    affinity: &WorkspaceAffinity,
) -> bool {
    !workspace.status.is_terminal()
        && workspace.worker_instance_id == affinity.worker_instance_id
        && workspace.attempt == affinity.attempt
        && workspace.version == affinity.version
}

/// Drain the notification-owned provider-effect outbox. Notification records deliberately share
/// worker provider execution with VM effects while retaining their own persistence receipt and
/// settlement path.
pub async fn run_notification_effect_dispatcher<T: NotificationStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance: String,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("notification effect dispatcher started");
    loop {
        let now = chrono::Utc::now();
        let policy = settings.current();
        match db
            .claim_pending_notification_effect_dispatches(
                instance.clone(),
                now,
                now + chrono::Duration::seconds(
                    policy.orchestration.action_dispatch_lease_seconds as i64,
                ),
                policy.orchestration.claim_batch_size as i64,
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
                                .mark_notification_effect_dispatch_published(dispatch.delivery_id)
                                .await
                            {
                                warn!(error = %err, delivery_id = %dispatch.delivery_id, "failed to acknowledge notification effect publication");
                            }
                        }
                        Err(err) => {
                            warn!(error = %err, delivery_id = %dispatch.delivery_id, "failed to publish notification effect");
                            let _ = db
                                .mark_notification_effect_dispatch_failed(
                                    dispatch.delivery_id,
                                    err.to_string(),
                                )
                                .await;
                        }
                    }
                }
            }
            Err(err) => warn!(error = %err, "failed to claim notification effect dispatches"),
        }
        tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.effect_dispatch_poll_interval_ms)) => {} }
    }
}

/// Periodically samples durable operational state so an idle deployment still has useful gauges.
/// This deliberately queries only aggregate queue/fleet state and never emits record identities.
pub async fn run_operational_metrics_sampler<
    T: RuntimeStore + WorkflowVmStore + NotificationStore + OrgStore + ReplicaStore,
>(
    db: Arc<T>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("operational metrics sampler started");
    loop {
        let started = std::time::Instant::now();
        let now = chrono::Utc::now();
        let policy = settings.current();
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

        let stale_before =
            now - chrono::Duration::seconds(policy.replicas.stale_after_seconds as i64);
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
            _ = tokio::time::sleep(Duration::from_secs(policy.orchestration.operational_metrics_interval_seconds)) => {}
        }
    }
}

/// periodically mark replicas offline once they have gone quiet past the inactivity window, then
/// hard-delete rows that have stayed quiet far longer so offline replicas do not pile up forever.
/// the operator-facing views derive stale state per fetch; this loop is the durable cleanup that
/// retires replicas that never sent an offline notice (e.g. crashed or evicted pods).
pub async fn run_replica_reaper<T: ReplicaStore>(
    db: Arc<T>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("replica reaper started");
    let registry = ReplicaRegistry::new(db);
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match registry
            .reap_inactive_after(policy.replicas.reap_after_seconds as i64)
            .await
        {
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
        match registry
            .delete_expired_after(policy.replicas.delete_after_seconds as i64)
            .await
        {
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
        match registry
            .prune_samples_after(policy.replicas.sample_retention_seconds as i64)
            .await
        {
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
            _ = tokio::time::sleep(Duration::from_secs(policy.replicas.reaper_interval_seconds)) => {}
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

pub async fn run_workspace_reconciler<T: WorkspaceStore + ReplicaStore>(
    db: Arc<T>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("workspace locality reconciler started");
    let operations = WorkspaceOperations::new(db);
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let succeeded = match operations
            .reconcile_expired(
                chrono::Utc::now(),
                None,
                policy.orchestration.claim_batch_size as i64,
            )
            .await
        {
            Ok(outcomes) => {
                for outcome in outcomes {
                    match outcome {
                        WorkspaceRecovery::Rebound(workspace) => info!(
                            workspace_id = %workspace.id,
                            worker_instance = %workspace.worker_instance_id,
                            "workspace rebound to returned worker instance"
                        ),
                        WorkspaceRecovery::Waiting(workspace) => info!(
                            workspace_id = %workspace.id,
                            worker_instance = %workspace.worker_instance_id,
                            "workspace waiting for its worker recovery grace"
                        ),
                        WorkspaceRecovery::Abandoned(workspace) => warn!(
                            workspace_id = %workspace.id,
                            admission_id = %workspace.admission_id,
                            scope = %workspace.scope,
                            attempt = workspace.attempt,
                            "workspace abandoned; admission coordinator must reschedule its scope"
                        ),
                    }
                }
                true
            }
            Err(error) => {
                warn!(error = %error, "workspace reconciliation failed");
                false
            }
        };
        stability::loop_iteration("workspace_reconciler", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = tokio::time::sleep(Duration::from_secs(policy.orchestration.workspace_reconcile_interval_seconds)) => {}
        }
    }
}

pub async fn run_usage_sampler<T: OrgStore>(
    db: Arc<T>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("usage sampler started");
    loop {
        let policy = settings.current();
        let interval = Duration::from_secs(policy.orchestration.usage_sample_interval_seconds);
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match db.list_all_resource_groups().await {
            Ok(groups) => {
                // bucket the timestamp to the sampling-interval boundary so every instance sampling
                // the same window produces the same (org, backend, kind, sampled_at) key; the insert
                // is an idempotent DO-NOTHING upsert, so N-up sampling converges to one row per
                // window instead of over-counting node-hours by the instance count.
                let now = bucket_to_interval(chrono::Utc::now(), interval);
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
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

/// periodically turn due workflow triggers into runs (formerly a waker loop, now in-process).
pub async fn run_trigger_loop<
    T: RuntimeStore + DefinitionStore + ScheduleStore + WorkflowVmStore,
>(
    db: Arc<T>,
    events: EventSender,
    instance_id: String,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("trigger firing loop started");
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let mut succeeded = true;
        match repository::claim_due_workflow_trigger_firings(
            db.as_ref(),
            instance_id.clone(),
            policy.orchestration.claim_batch_size as i64,
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
            policy.orchestration.claim_batch_size as i64,
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
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.trigger_poll_interval_ms)) => {}
        }
    }
}

/// drain the durable replica-directive outbox, with periodic redelivery as a reconnect backstop.
pub async fn run_agent_directive_publisher<T: ReplicaStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance_id: String,
    agent_nudge: Arc<Notify>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("agent directive publisher started");
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let succeeded = if let Err(err) = repository::publish_due_agent_directives(
            db.as_ref(),
            broker.as_ref(),
            &instance_id,
            policy.orchestration.claim_batch_size as i64,
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
            _ = tokio::time::sleep(Duration::from_millis(policy.orchestration.agent_directive_poll_interval_ms)) => {}
        }
    }
}
