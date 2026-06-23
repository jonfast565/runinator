use std::{path::PathBuf, time::Duration};

use clap::Parser;
use runinator_db_cli::DatabaseBackend;
use runinator_models::errors::SendableError;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Archive and delete old Runinator database rows."
)]
pub struct Cli {
    #[arg(long, env = "RUNINATOR_DATABASE", value_enum)]
    pub database: DatabaseBackend,

    #[arg(long, env = "RUNINATOR_DATABASE_URL")]
    pub database_url: Option<String>,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVE_DIR",
        default_value = "/var/lib/runinator/archive"
    )]
    pub archive_dir: PathBuf,

    #[arg(long, env = "RUNINATOR_ARCHIVER_INTERVAL", default_value = "1h")]
    pub interval: String,

    #[arg(long, env = "RUNINATOR_ARCHIVER_CLAIM_LEASE", default_value = "10m")]
    pub claim_lease: String,

    #[arg(long, env = "RUNINATOR_ARCHIVER_BATCH_SIZE", default_value_t = 1000)]
    pub batch_size: i64,

    #[arg(long, env = "RUNINATOR_ARCHIVER_DRY_RUN", default_value_t = false)]
    pub dry_run: bool,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_WORKFLOW_RUN_RETENTION",
        default_value = "90d"
    )]
    pub workflow_run_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_NODE_LOG_RETENTION",
        default_value = "30d"
    )]
    pub node_log_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_READY_NODE_RETENTION",
        default_value = "30d"
    )]
    pub ready_node_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_PUBLISHED_DISPATCH_RETENTION",
        default_value = "7d"
    )]
    pub published_dispatch_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_READ_NOTIFICATION_RETENTION",
        default_value = "30d"
    )]
    pub read_notification_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_DEAD_LETTER_RETENTION",
        default_value = "90d"
    )]
    pub dead_letter_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_AUDIT_LOG_RETENTION",
        default_value = "365d"
    )]
    pub audit_log_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_IDEMPOTENCY_RETENTION",
        default_value = "7d"
    )]
    pub idempotency_retention: String,

    /// path to a file that is touched every 30 seconds to signal liveness; used with k8s exec.
    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_LIVENESS_FILE",
        default_value = "/tmp/runinator-archiver-liveness"
    )]
    pub liveness_file: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database: DatabaseBackend,
    pub database_url: String,
    pub archive_dir: PathBuf,
    pub interval: Duration,
    pub claim_lease: Duration,
    pub batch_size: i64,
    pub dry_run: bool,
    pub workflow_run_retention: Option<Duration>,
    pub node_log_retention: Option<Duration>,
    pub ready_node_retention: Option<Duration>,
    pub published_dispatch_retention: Option<Duration>,
    pub read_notification_retention: Option<Duration>,
    pub dead_letter_retention: Option<Duration>,
    pub audit_log_retention: Option<Duration>,
    pub idempotency_retention: Option<Duration>,
    pub liveness_file: String,
}

impl Config {
    pub fn from_cli(cli: Cli) -> Result<Self, SendableError> {
        let database_url = cli
            .database_url
            .or_else(|| std::env::var("DATABASE_URL").ok())
            .ok_or_else(|| -> SendableError {
                "missing connection string: pass --database-url or set RUNINATOR_DATABASE_URL"
                    .into()
            })?;
        Ok(Self {
            database: cli.database,
            database_url,
            archive_dir: cli.archive_dir,
            interval: parse_required_duration(&cli.interval)?,
            claim_lease: parse_required_duration(&cli.claim_lease)?,
            batch_size: cli.batch_size.max(1),
            dry_run: cli.dry_run,
            workflow_run_retention: parse_optional_duration(&cli.workflow_run_retention)?,
            node_log_retention: parse_optional_duration(&cli.node_log_retention)?,
            ready_node_retention: parse_optional_duration(&cli.ready_node_retention)?,
            published_dispatch_retention: parse_optional_duration(
                &cli.published_dispatch_retention,
            )?,
            read_notification_retention: parse_optional_duration(&cli.read_notification_retention)?,
            dead_letter_retention: parse_optional_duration(&cli.dead_letter_retention)?,
            audit_log_retention: parse_optional_duration(&cli.audit_log_retention)?,
            idempotency_retention: parse_optional_duration(&cli.idempotency_retention)?,
            liveness_file: cli.liveness_file,
        })
    }
}

pub fn parse_optional_duration(value: &str) -> Result<Option<Duration>, SendableError> {
    let trimmed = value.trim().to_ascii_lowercase();
    if matches!(trimmed.as_str(), "off" | "none" | "disabled") {
        return Ok(None);
    }
    parse_required_duration(value).map(Some)
}

pub fn parse_required_duration(value: &str) -> Result<Duration, SendableError> {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err("duration cannot be empty".into());
    }
    let split_at = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(trimmed.len());
    if split_at == 0 || split_at == trimmed.len() {
        return Err(
            format!("invalid duration '{value}', expected values like 30m, 1h, 90d").into(),
        );
    }
    let amount = trimmed[..split_at].parse::<u64>()?;
    let unit = &trimmed[split_at..];
    let seconds = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => amount,
        "m" | "min" | "mins" | "minute" | "minutes" => amount * 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => amount * 60 * 60,
        "d" | "day" | "days" => amount * 60 * 60 * 24,
        "w" | "week" | "weeks" => amount * 60 * 60 * 24 * 7,
        _ => return Err(format!("unknown duration unit '{unit}' in '{value}'").into()),
    };
    Ok(Duration::from_secs(seconds.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_duration_units() {
        assert_eq!(parse_required_duration("30m").unwrap().as_secs(), 1800);
        assert_eq!(parse_required_duration("1h").unwrap().as_secs(), 3600);
        assert_eq!(parse_required_duration("2w").unwrap().as_secs(), 1_209_600);
    }

    #[test]
    fn parses_disabled_retention() {
        assert!(parse_optional_duration("off").unwrap().is_none());
        assert!(parse_optional_duration("none").unwrap().is_none());
    }
}
