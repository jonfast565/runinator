mod config;
mod errors;
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
use log::{error, info, warn};
use runinator_api::{
    AsyncApiClient, ReplicaServiceConfig, ReplicaSession, StaticLocator, register_replica_session,
    spawn_replica_heartbeat_with_telemetry,
};
use runinator_database::{
    archive::{ArchiveRow, ArchiveTable},
    interfaces::DatabaseImpl,
};
use runinator_db_cli::dispatch_database;
use runinator_models::errors::SendableError;
use runinator_models::replicas::ReplicaKind;
use runinator_utilities::resource_telemetry::{TelemetryCollector, attributes_with_host_metadata};
use serde_json::json;
use uuid::Uuid;

use crate::config::{Cli, Config};

const ARCHIVE_FILE_EXTENSION: &str = "jsonl.gz";

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::var_os("RUST_LOG").is_none() {
        unsafe {
            std::env::set_var("RUST_LOG", "info");
        }
    }
    env_logger::init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("Archiver failed: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), SendableError> {
    let config = Config::from_cli(Cli::parse())?;
    dispatch_database!(
        config.database,
        sqlite: config.database_url.clone(),
        url: config.database_url.clone(),
        |db| { run_loop(db, config).await }
    )
}

async fn run_loop<T: DatabaseImpl>(db: Arc<T>, config: Config) -> Result<(), SendableError> {
    fs::create_dir_all(&config.archive_dir)?;
    let archiver_id = format!("runinator-archiver-{}", Uuid::new_v4());
    info!("Runinator archiver started as {archiver_id}");
    let shutdown = Arc::new(Notify::new());
    spawn_liveness(&config, shutdown.clone());
    // registration is optional: only when a web service url is configured does the archiver appear in
    // the replica list and heartbeat. held for the loop lifetime so the heartbeat keeps running.
    let _heartbeat = match register_replica(&config, &archiver_id, shutdown.clone()).await? {
        Registration::Heartbeat(handle) => Some(handle),
        Registration::Disabled => None,
        Registration::Shutdown => return Ok(()),
    };
    loop {
        if let Err(err) = run_once(db.as_ref(), &config, &archiver_id).await {
            error!("Archiver pass failed: {err}");
        }
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(err) = result {
                    warn!("Failed to listen for shutdown signal: {err}");
                }
                info!("Runinator archiver shutting down");
                shutdown.notify_waiters();
                return Ok(());
            }
            _ = tokio::time::sleep(config.interval) => {}
        }
    }
}

// touches the configured liveness file on an interval until shutdown; used by the k8s exec probe.
fn spawn_liveness(config: &Config, shutdown: Arc<Notify>) -> Option<tokio::task::JoinHandle<()>> {
    runinator_utilities::liveness::spawn_liveness(
        &config.liveness_file,
        runinator_utilities::liveness::DEFAULT_LIVENESS_INTERVAL,
        shutdown,
    )
}

// outcome of the optional replica registration at startup.
enum Registration {
    // registration succeeded; the heartbeat task keeps the replica live.
    Heartbeat(tokio::task::JoinHandle<()>),
    // no web service url configured; the archiver runs database-only.
    Disabled,
    // ctrl_c arrived during registration; the caller should exit cleanly.
    Shutdown,
}

// register the archiver in the replica list when a web service url is configured, retrying with
// backoff and staying interruptible so ctrl_c during startup still shuts the process down.
async fn register_replica(
    config: &Config,
    archiver_id: &str,
    shutdown: Arc<Notify>,
) -> Result<Registration, SendableError> {
    let Some(api_base_url) = config.api_base_url.as_deref() else {
        info!("No web service url configured; running database-only without replica registration");
        return Ok(Registration::Disabled);
    };
    let api_client = AsyncApiClient::with_credentials(
        StaticLocator::new(api_base_url.to_string()),
        config.api_key.clone(),
    )
    .map_err(|err| errors::REPLICA_REGISTER.error(err))?;
    let service_config = ReplicaServiceConfig {
        replica_type: ReplicaKind::Archiver,
        instance_id: archiver_id.to_string(),
        display_name: Some(archiver_id.to_string()),
        host: config.advertise_host.clone(),
        port: None,
        base_path: None,
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        attributes: attributes_with_host_metadata(&runinator_models::json!({
            "archive_dir": config.archive_dir.display().to_string(),
        })),
        heartbeat_interval: Duration::from_secs(10),
    };
    let session = tokio::select! {
        result = register_archiver_replica_with_retry(&api_client, &service_config) => result?,
        signal = tokio::signal::ctrl_c() => {
            if let Err(err) = signal {
                warn!("Failed to listen for shutdown signal: {err}");
            }
            info!("Shutdown signal received before archiver registration completed");
            return Ok(Registration::Shutdown);
        }
    };
    Ok(Registration::Heartbeat(
        spawn_replica_heartbeat_with_telemetry(
            api_client.clone(),
            session,
            shutdown,
            Some(Arc::new(TelemetryCollector::new())),
        ),
    ))
}

// registration retry envelope: archiver startup keeps trying while the web service is briefly
// unreachable, then gives up so the process exits non-zero and the orchestrator restarts it.
const REGISTER_MAX_ATTEMPTS: u32 = 8;
const REGISTER_BASE_BACKOFF: Duration = Duration::from_secs(2);
const REGISTER_MAX_BACKOFF: Duration = Duration::from_secs(30);

// exponential backoff for the nth registration attempt (1-based), capped at REGISTER_MAX_BACKOFF.
fn register_backoff(attempt: u32) -> Duration {
    let factor = 1u32
        .checked_shl(attempt.saturating_sub(1))
        .unwrap_or(u32::MAX);
    REGISTER_BASE_BACKOFF
        .saturating_mul(factor)
        .min(REGISTER_MAX_BACKOFF)
}

// register with bounded retries and loud logging, returning an error once attempts are exhausted so
// the archiver fails visibly instead of running unregistered.
async fn register_archiver_replica_with_retry(
    api_client: &AsyncApiClient<StaticLocator>,
    service_config: &ReplicaServiceConfig,
) -> Result<ReplicaSession, SendableError> {
    let mut attempt = 1;
    loop {
        match register_replica_session(api_client, service_config.clone()).await {
            Ok(session) => {
                if attempt > 1 {
                    info!("Archiver replica registered on attempt {}", attempt);
                }
                return Ok(session);
            }
            Err(err) if attempt >= REGISTER_MAX_ATTEMPTS => {
                error!(
                    "Failed to register archiver replica after {} attempts, giving up: {}",
                    attempt, err
                );
                return Err(errors::REPLICA_REGISTER.error(err));
            }
            Err(err) => {
                let backoff = register_backoff(attempt);
                error!(
                    "Failed to register archiver replica (attempt {}/{}), retrying in {}s: {}",
                    attempt,
                    REGISTER_MAX_ATTEMPTS,
                    backoff.as_secs(),
                    err
                );
                tokio::time::sleep(backoff).await;
                attempt += 1;
            }
        }
    }
}

async fn run_once<T: DatabaseImpl>(
    db: &T,
    config: &Config,
    archiver_id: &str,
) -> Result<(), SendableError> {
    mark_all(db, config).await?;
    let now = Utc::now();
    let lease = chrono_from_std(config.claim_lease)?;
    let marks = db
        .claim_archive_marks(archiver_id.to_string(), now, now + lease, config.batch_size)
        .await?;
    if marks.is_empty() {
        return Ok(());
    }
    let mark_ids = marks.iter().map(|mark| mark.id).collect::<Vec<_>>();
    let rows = match db.fetch_archive_rows(marks).await {
        Ok(rows) => rows,
        Err(err) => {
            db.fail_archive_marks(mark_ids, err.to_string()).await?;
            return Err(err);
        }
    };
    if rows.is_empty() {
        db.complete_archive_marks(mark_ids).await?;
        return Ok(());
    }
    if config.dry_run {
        info!("Dry run: would archive {} row(s)", rows.len());
        db.fail_archive_marks(mark_ids, "dry run; no rows deleted".into())
            .await?;
        return Ok(());
    }
    if let Err(err) = write_archive_jsonl_files(&config.archive_dir, &rows) {
        db.fail_archive_marks(mark_ids, err.to_string()).await?;
        return Err(err);
    }
    let archived_mark_ids = rows.iter().map(|row| row.mark_id).collect::<Vec<_>>();
    db.delete_archive_rows(rows).await?;
    db.complete_archive_marks(archived_mark_ids).await?;
    Ok(())
}

async fn mark_all<T: DatabaseImpl>(db: &T, config: &Config) -> Result<(), SendableError> {
    let policies = [
        (ArchiveTable::WorkflowRuns, config.workflow_run_retention),
        (ArchiveTable::WorkflowNodeChunks, config.node_log_retention),
        (
            ArchiveTable::WorkflowReadyNodes,
            config.ready_node_retention,
        ),
        (ArchiveTable::RunChunks, config.node_log_retention),
        (
            ArchiveTable::WorkflowActionDispatches,
            config.published_dispatch_retention,
        ),
        (
            ArchiveTable::Notifications,
            config.read_notification_retention,
        ),
        (ArchiveTable::DeadLetters, config.dead_letter_retention),
        (ArchiveTable::AuditLog, config.audit_log_retention),
        (ArchiveTable::IdempotencyKeys, config.idempotency_retention),
    ];
    for (table, retention) in policies {
        let Some(retention) = retention else {
            continue;
        };
        let cutoff = Utc::now() - chrono_from_std(retention)?;
        let count = db
            .mark_archive_candidates(table, cutoff, config.batch_size)
            .await?;
        if count > 0 {
            info!("Marked {count} {table} row(s) for archival");
        }
    }
    Ok(())
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
        info!("Wrote archive {}", final_path.display());
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
