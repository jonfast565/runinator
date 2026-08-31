mod config;
mod errors;
mod service;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

use tokio::sync::Notify;

use chrono::{Duration as ChronoDuration, Utc};
use clap::Parser;
use flate2::{Compression, write::GzEncoder};
use runinator_broker::{
    Broker, BrokerClientConfig, BrokerConnectionMode, BrokerConsumerProfile, IngressMessage,
    select_broker_connection,
};
use runinator_comm::WsIngressCommand;
use runinator_db_cli::dispatch_database;
use runinator_models::replicas::{ReplicaKind, ReplicaRegistrationRequest};
use runinator_models::{errors::SendableError, server_settings::ArchiverSettings};
use runinator_observability::resource_telemetry::{
    TelemetryCollector, attributes_with_host_metadata, attributes_with_telemetry,
};
use runinator_store::{
    archive::{ArchiveRow, ArchiveTable},
    roles::{ArchiveStore, SettingStore},
};
use serde_json::json;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::{Cli, Config};
use service::ArchiverService;

const ARCHIVE_FILE_EXTENSION: &str = "jsonl.gz";

#[tokio::main]
async fn main() -> ExitCode {
    ArchiverService::new().run().await
}

async fn run_process() -> ExitCode {
    // held for the process lifetime so otel signals flush on shutdown. shares the same
    // Use the same RUNINATOR_LOG tracing, file, and OpenTelemetry pipeline as the other services.
    let _telemetry = match runinator_platform::startup::startup("Runinator Archiver") {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("Archiver startup failed: {err}");
            return ExitCode::FAILURE;
        }
    };

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!(
                error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
                "archiver failed: {err}"
            );
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), SendableError> {
    let config = Config::from_cli(Cli::parse())?;
    let broker_mode = BrokerConnectionMode::parse(&config.broker_mode)
        .ok_or_else(|| format!("unknown --broker-mode '{}'", config.broker_mode))?;
    let connection = select_broker_connection(
        broker_mode,
        BrokerClientConfig {
            backend: config.broker_backend.clone(),
            endpoint: config.broker_endpoint.clone(),
            effect_topic: config.broker_effect_topic.clone(),
            infrastructure_effect_topic: config.broker_infrastructure_effect_topic.clone(),
            control_topic: config.broker_control_topic.clone(),
            agent_topic: None,
            effect_result_topic: config.broker_effect_result_topic.clone(),
            client_id: config.broker_client_id.clone(),
            relay_credential: config.api_key.clone(),
            // Direct broker adapters initialize the ingress topology only when both orchestration
            // names are supplied. The archiver publishes ingress only and never consumes wakes.
            wake_topic: Some(config.broker_wake_topic.clone()),
            ingress_topic: Some(config.broker_ingress_topic.clone()),
        },
        config.service_url.clone().unwrap_or_default(),
        Some(&config.broker_relay_path),
    );
    let broker_connection = connection.description()?;
    let broker_backend = connection.client_config()?.backend;
    let broker = connection
        .connect(BrokerConsumerProfile::IngressPublisher)
        .await?;
    dispatch_database!(
        config.database,
        sqlite: config.database_url.clone(),
        url: config.database_url.clone(),
        |db| {
            run_loop(
                db,
                broker.clone(),
                config,
                broker_backend,
                broker_connection,
            )
            .await
        }
    )
}

async fn run_loop<T: ArchiveStore + SettingStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    config: Config,
    broker_backend: String,
    broker_connection: String,
) -> Result<(), SendableError> {
    fs::create_dir_all(&config.archive_dir)?;
    let archiver_id = format!("runinator-archiver-{}", Uuid::new_v4());
    info!(archiver_id = %archiver_id, "archiver started");
    let shutdown = Arc::new(Notify::new());
    spawn_liveness(&config, shutdown.clone());
    let replica_id = Uuid::now_v7();
    let runtime_id = replica_id.to_string();
    let base_attributes = attributes_with_host_metadata(&runinator_models::json!({
        "archive_dir": config.archive_dir.display().to_string(),
        "broker_backend": broker_backend,
        "broker_connection": broker_connection,
        "broker_client_id": config.broker_client_id.clone(),
    }));
    publish_replica_availability(
        broker.as_ref(),
        replica_id,
        &runtime_id,
        &archiver_id,
        config.advertise_host.clone(),
        base_attributes.clone(),
    )
    .await?;
    let heartbeat = spawn_replica_heartbeat(
        broker.clone(),
        replica_id,
        runtime_id,
        archiver_id.clone(),
        config.advertise_host.clone(),
        base_attributes,
        shutdown.clone(),
    );
    let bootstrap_policy = config.bootstrap_archiver_settings();
    let mut policy = load_archiver_policy(db.as_ref(), &bootstrap_policy).await?;
    loop {
        let pass_started = std::time::Instant::now();
        if let Err(err) = run_once(db.as_ref(), &config.archive_dir, &policy, &archiver_id).await {
            error!(
                error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
                "archiver pass failed: {err}"
            );
        }
        loop {
            let elapsed = pass_started.elapsed();
            let interval = Duration::from_secs(policy.interval_seconds);
            if elapsed >= interval {
                break;
            }
            let remaining = interval.saturating_sub(elapsed);
            let refresh_delay = remaining.min(Duration::from_secs(30));
            tokio::select! {
                result = tokio::signal::ctrl_c() => {
                    if let Err(err) = result {
                        warn!("failed to listen for shutdown signal: {err}");
                    }
                    info!("archiver shutting down");
                    shutdown.notify_waiters();
                    let _ = heartbeat.await;
                    return Ok(());
                }
                _ = tokio::time::sleep(refresh_delay) => {
                    match load_archiver_policy(db.as_ref(), &bootstrap_policy).await {
                        Ok(next) => policy = next,
                        Err(err) => error!(
                            error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
                            "failed to refresh archiver settings: {err}"
                        ),
                    }
                }
            }
        }
    }
}

async fn load_archiver_policy<T: SettingStore>(
    db: &T,
    bootstrap: &ArchiverSettings,
) -> Result<ArchiverSettings, SendableError> {
    Ok(
        runinator_engine::settings::load_persisted_server_settings(db)
            .await?
            .map_or_else(|| bootstrap.clone(), |settings| settings.archiver),
    )
}

// Touch the liveness file until shutdown for the Kubernetes exec probe.
fn spawn_liveness(config: &Config, shutdown: Arc<Notify>) -> Option<tokio::task::JoinHandle<()>> {
    runinator_platform::liveness::spawn_liveness(
        &config.liveness_file,
        runinator_platform::liveness::DEFAULT_LIVENESS_INTERVAL,
        shutdown,
    )
}

async fn publish_replica_availability(
    broker: &dyn Broker,
    replica_id: Uuid,
    runtime_id: &str,
    instance_id: &str,
    host: Option<String>,
    attributes: runinator_models::value::Value,
) -> Result<(), runinator_broker::BrokerError> {
    let command = WsIngressCommand::replica_available(
        ReplicaRegistrationRequest {
            replica_id: Some(replica_id),
            replica_type: ReplicaKind::Archiver,
            instance_id: instance_id.to_string(),
            runtime_id: runtime_id.to_string(),
            display_name: Some(instance_id.to_string()),
            host,
            port: None,
            base_path: None,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            attributes,
        },
        Vec::new(),
    );
    broker
        .publish_ingress(IngressMessage {
            dedupe_key: Some(command.dedupe_key()),
            command,
            enqueued_at: Utc::now(),
        })
        .await
}

fn spawn_replica_heartbeat(
    broker: Arc<dyn Broker>,
    replica_id: Uuid,
    runtime_id: String,
    instance_id: String,
    host: Option<String>,
    base_attributes: runinator_models::value::Value,
    shutdown: Arc<Notify>,
) -> tokio::task::JoinHandle<()> {
    let telemetry = TelemetryCollector::new();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = shutdown.notified() => {
                    let command = WsIngressCommand::replica_offline(replica_id, runtime_id.clone());
                    let _ = broker.publish_ingress(IngressMessage {
                        dedupe_key: Some(command.dedupe_key()),
                        command,
                        enqueued_at: Utc::now(),
                    }).await;
                    return;
                }
                _ = ticker.tick() => {
                    let attributes = attributes_with_telemetry(&base_attributes, &telemetry);
                    if let Err(err) = publish_replica_availability(
                        broker.as_ref(), replica_id, &runtime_id, &instance_id, host.clone(), attributes,
                    ).await {
                        error!("failed to announce archiver availability: {err}");
                    }
                }
            }
        }
    })
}

async fn run_once<T: ArchiveStore>(
    db: &T,
    archive_dir: &Path,
    policy: &ArchiverSettings,
    archiver_id: &str,
) -> Result<(), SendableError> {
    if !policy.dry_run {
        prune_housekeeping(db, policy).await?;
    }
    loop {
        let marked = mark_all(db, policy).await?;
        let processed = archive_one_batch(db, archive_dir, policy, archiver_id).await?;
        if policy.dry_run || (!processed && marked == 0) {
            return Ok(());
        }
    }
}

async fn archive_one_batch<T: ArchiveStore>(
    db: &T,
    archive_dir: &Path,
    policy: &ArchiverSettings,
    archiver_id: &str,
) -> Result<bool, SendableError> {
    let now = Utc::now();
    let lease = ChronoDuration::seconds(policy.claim_lease_seconds as i64);
    let batch_size = policy.batch_size as i64;
    let marks = db
        .claim_archive_marks(archiver_id.to_string(), now, now + lease, batch_size)
        .await?;
    if marks.is_empty() {
        return Ok(false);
    }
    let mark_ids = marks.iter().map(|mark| mark.id).collect::<Vec<_>>();
    let rows = match db.fetch_archive_rows(marks).await {
        Ok(rows) => rows,
        Err(err) => {
            error!(
                marks = mark_ids.len(),
                error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
                "failed to fetch archive rows: {err}"
            );
            db.fail_archive_marks(mark_ids, err.to_string()).await?;
            return Err(err);
        }
    };
    if rows.is_empty() {
        db.complete_archive_marks(mark_ids).await?;
        return Ok(true);
    }
    if policy.dry_run {
        info!(rows = rows.len(), "dry run: would archive row(s)");
        db.fail_archive_marks(mark_ids, "dry run; no rows deleted".into())
            .await?;
        return Ok(true);
    }
    if let Err(err) = write_archive_jsonl_files(archive_dir, &rows) {
        error!(
            rows = rows.len(),
            error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
            "failed to write archive file(s): {err}"
        );
        db.fail_archive_marks(mark_ids, err.to_string()).await?;
        return Err(err);
    }
    let archived_count = rows.len();
    db.delete_archive_rows(rows).await?;
    // A claimed source can disappear through an explicit delete or a parent cascade after it was
    // marked. Complete every claim after the rows that still exist are safely archived so a mixed
    // present/missing batch cannot leave an immortal mark at the head of the queue.
    db.complete_archive_marks(mark_ids).await?;
    info!(rows = archived_count, "archived row(s)");
    Ok(true)
}

async fn mark_all<T: ArchiveStore>(
    db: &T,
    policy: &ArchiverSettings,
) -> Result<u64, SendableError> {
    let policies: [(ArchiveTable, Option<Duration>); ArchiveTable::ALL.len()] = [
        (
            ArchiveTable::RunChunks,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::RunArtifacts,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::Runs,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowEffectOutputEvents,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowEffectDispatches,
            retention(policy.effect_dispatch_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowEffects,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowJournalEntries,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowContinuations,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowVmModules,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowFiles,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::PipelineMemberAttempts,
            retention(policy.pipeline_run_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowRuns,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowTriggerFirings,
            retention(policy.workflow_run_retention_seconds),
        ),
        (
            ArchiveTable::PipelineTriggerFirings,
            retention(policy.pipeline_run_retention_seconds),
        ),
        (
            ArchiveTable::PipelineRuns,
            retention(policy.pipeline_run_retention_seconds),
        ),
        (
            ArchiveTable::PipelineRevisions,
            retention(policy.revision_retention_seconds),
        ),
        (
            ArchiveTable::OrchestrationPendingIntents,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::OrchestrationCommands,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::OrchestrationEvidence,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::ExternalOperations,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::WorkspaceLeases,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::OrchestrationCorrelationAliases,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::OrchestrationEventReductions,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::OrchestrationEpochs,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::IngressEvents,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::OrchestrationBindings,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::IngressAdmissions,
            retention(policy.orchestration_retention_seconds),
        ),
        (
            ArchiveTable::NotificationDeliveries,
            retention(policy.notification_retention_seconds),
        ),
        (
            ArchiveTable::Notifications,
            retention(policy.notification_retention_seconds),
        ),
        (
            ArchiveTable::AutomationRecords,
            retention(policy.automation_retention_seconds),
        ),
        (
            ArchiveTable::Gates,
            retention(policy.automation_retention_seconds),
        ),
        (
            ArchiveTable::OrgUsageLedger,
            retention(policy.usage_retention_seconds),
        ),
        (
            ArchiveTable::WorkflowRevisions,
            retention(policy.revision_retention_seconds),
        ),
        (
            ArchiveTable::AgentDirectives,
            retention(policy.agent_directive_retention_seconds),
        ),
        (
            ArchiveTable::DeadLetters,
            retention(policy.dead_letter_retention_seconds),
        ),
        (
            ArchiveTable::AuditLog,
            retention(policy.audit_log_retention_seconds),
        ),
        (
            ArchiveTable::IdempotencyKeys,
            retention(policy.idempotency_retention_seconds),
        ),
    ];
    let mut marked = 0;
    for (table, retention) in policies {
        let Some(retention) = retention else {
            continue;
        };
        let cutoff = Utc::now() - chrono_from_std(retention)?;
        let count = db
            .mark_archive_candidates(table, cutoff, policy.batch_size as i64)
            .await?;
        if count > 0 {
            info!(table = %table, count, "marked row(s) for archival");
        }
        marked += count;
    }
    Ok(marked)
}

async fn prune_housekeeping<T: ArchiveStore>(
    db: &T,
    policy: &ArchiverSettings,
) -> Result<(), SendableError> {
    let now = Utc::now();
    let batch_size = policy.batch_size as i64;
    if let Some(retention) = retention(policy.archive_ledger_retention_seconds) {
        loop {
            let count = db
                .prune_completed_archive_marks(now - chrono_from_std(retention)?, batch_size)
                .await?;
            if count > 0 {
                info!(count, "pruned completed archive mark(s)");
            }
            if count < policy.batch_size {
                break;
            }
        }
    }
    if let Some(retention) = retention(policy.security_retention_seconds) {
        loop {
            let count = db
                .prune_expired_security_records(now - chrono_from_std(retention)?, batch_size)
                .await?;
            if count > 0 {
                info!(count, "pruned expired security record(s)");
            }
            if count < policy.batch_size {
                break;
            }
        }
    }
    if let Some(retention) = retention(policy.coordination_retention_seconds) {
        let cutoff = now - chrono_from_std(retention)?;
        loop {
            let count = db.prune_workflow_cooldowns(cutoff, batch_size).await?;
            if count > 0 {
                info!(count, "pruned workflow cooldown(s)");
            }
            if count < policy.batch_size {
                break;
            }
        }
        loop {
            let count = db.prune_workflow_mutexes(cutoff, batch_size).await?;
            if count > 0 {
                info!(count, "pruned inactive workflow mutex(es)");
            }
            if count < policy.batch_size {
                break;
            }
        }
    }
    Ok(())
}

fn retention(seconds: u64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

fn write_archive_jsonl_files(root: &Path, rows: &[ArchiveRow]) -> Result<(), SendableError> {
    let mut groups = BTreeMap::<(String, ArchiveTable), Vec<&ArchiveRow>>::new();
    for row in rows {
        groups
            .entry((row.created_at.format("%F").to_string(), row.table))
            .or_default()
            .push(row);
    }
    for ((day, table), rows) in groups {
        let dir = root.join(&day);
        fs::create_dir_all(&dir)?;
        let final_path = dir.join(format!(
            "{table}-{}.{}",
            Uuid::new_v4(),
            ARCHIVE_FILE_EXTENSION
        ));
        let tmp_path = temp_path(&final_path);
        let file = File::create(&tmp_path)?;
        let mut encoder = GzEncoder::new(BufWriter::new(file), Compression::default());
        let archived_at = Utc::now().to_rfc3339();
        for row in rows {
            let line = json!({
                "schema_version": 1,
                "archived_at": archived_at,
                "source_table": row.table.as_str(),
                "primary_key": { "id": row.primary_key.to_string() },
                "created_at": row.created_at.timestamp(),
                "row": row.row,
            });
            serde_json::to_writer(&mut encoder, &line)?;
            encoder.write_all(b"\n")?;
        }
        encoder.finish()?;
        fs::rename(&tmp_path, &final_path)?;
        info!(path = %final_path.display(), table = %table, "wrote archive file");
    }
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".tmp");
    PathBuf::from(value)
}

fn chrono_from_std(duration: std::time::Duration) -> Result<ChronoDuration, SendableError> {
    ChronoDuration::from_std(duration)
        .map_err(|err| -> SendableError { Box::new(std::io::Error::other(err)) })
}
