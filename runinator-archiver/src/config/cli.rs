#[allow(unused_imports)]
use super::*;

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
        env = "RUNINATOR_ARCHIVER_PIPELINE_RUN_RETENTION",
        default_value = "90d"
    )]
    pub pipeline_run_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_EFFECT_DISPATCH_RETENTION",
        default_value = "7d"
    )]
    pub effect_dispatch_retention: String,

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

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_AUTOMATION_RETENTION",
        default_value = "90d"
    )]
    pub automation_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_USAGE_RETENTION",
        default_value = "365d"
    )]
    pub usage_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_REVISION_RETENTION",
        default_value = "365d"
    )]
    pub revision_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_AGENT_DIRECTIVE_RETENTION",
        default_value = "30d"
    )]
    pub agent_directive_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_LEDGER_RETENTION",
        default_value = "30d"
    )]
    pub archive_ledger_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_SECURITY_RETENTION",
        default_value = "7d"
    )]
    pub security_retention: String,

    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_COOLDOWN_RETENTION",
        default_value = "30d"
    )]
    pub cooldown_retention: String,

    /// File touched every 30 seconds for the Kubernetes exec probe.
    #[arg(
        long,
        env = "RUNINATOR_ARCHIVER_LIVENESS_FILE",
        default_value = "/tmp/runinator-archiver-liveness"
    )]
    pub liveness_file: String,

    /// Broker transport used for lifecycle announcements.
    #[arg(long, env = "RUNINATOR_BROKER_BACKEND", default_value = "tcp")]
    pub broker_backend: String,

    /// Broker endpoint used for lifecycle announcements.
    #[arg(
        long,
        env = "RUNINATOR_BROKER_ENDPOINT",
        default_value = "127.0.0.1:7070"
    )]
    pub broker_endpoint: String,

    /// How this process reaches the broker: `direct` (the configured backend) or `relay`
    /// (through the web service's authenticated WebSocket endpoint).
    #[arg(long, env = "RUNINATOR_BROKER_MODE", default_value = "direct")]
    pub broker_mode: String,

    /// Web-service base URL used only with `--broker-mode relay`.
    #[arg(long, env = "RUNINATOR_SERVICE_URL")]
    pub service_url: Option<String>,

    /// Bearer credential for the WebSocket relay, used only with `--broker-mode relay`.
    #[arg(long, env = "RUNINATOR_API_KEY")]
    pub api_key: Option<String>,

    /// Relay path relative to `--service-url`; override during a staged endpoint migration.
    #[arg(
        long,
        env = "RUNINATOR_BROKER_RELAY_PATH",
        default_value = DEFAULT_BROKER_RELAY_PATH
    )]
    pub broker_relay_path: String,

    #[arg(long, default_value = "runinator.effects")]
    pub broker_effect_topic: String,

    #[arg(long, default_value = "runinator.effects.infrastructure")]
    pub broker_infrastructure_effect_topic: String,

    #[arg(long, default_value = "runinator.control")]
    pub broker_control_topic: String,

    #[arg(long, default_value = "runinator.effect-results")]
    pub broker_effect_result_topic: String,

    #[arg(long, default_value = "runinator.wake")]
    pub broker_wake_topic: String,

    #[arg(long, default_value = "runinator.ingress")]
    pub broker_ingress_topic: String,

    #[arg(long, default_value = "runinator-archiver")]
    pub broker_client_id: String,

    /// Stable address advertised to the replica list. In Kubernetes, this is the pod's DNS name.
    #[arg(long, env = "RUNINATOR_ADVERTISE_HOST")]
    pub advertise_host: Option<String>,
}
