mod config;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use tokio::sync::Notify;

use chrono::{Duration as ChronoDuration, Utc};
use clap::Parser;
use flate2::{Compression, write::GzEncoder};
use log::{error, info, warn};
use runinator_database::{
    archive::{ArchiveRow, ArchiveTable},
    interfaces::DatabaseImpl,
};
use runinator_db_cli::dispatch_database;
use runinator_models::errors::SendableError;
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
