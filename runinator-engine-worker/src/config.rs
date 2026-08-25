use std::path::PathBuf;

use clap::Parser;
use runinator_broker::DEFAULT_BROKER_RELAY_PATH;
use runinator_db_cli::DatabaseBackend;

/// Command-line configuration for the standalone engine worker.
/// It mirrors the web service's database and broker options, without the HTTP surface.
#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct CliArgs {
    /// Show a local full-screen runtime dashboard instead of streaming logs to stdout.
    #[arg(long, env = "RUNINATOR_TUI", default_value_t = false)]
    pub tui: bool,

    /// Database backend to use. Also reads RUNINATOR_DATABASE.
    #[arg(
        long,
        env = "RUNINATOR_DATABASE",
        value_enum,
        default_value_t = DatabaseBackend::Sqlite
    )]
    pub database: DatabaseBackend,

    /// Path to the SQLite database file (used when --database=sqlite). Also reads RUNINATOR_SQLITE_PATH.
    #[arg(long, env = "RUNINATOR_SQLITE_PATH")]
    pub sqlite_path: Option<PathBuf>,

    /// Connection string for Postgres/MySQL/MariaDB. Also reads RUNINATOR_DATABASE_URL.
    #[arg(long, env = "RUNINATOR_DATABASE_URL")]
    pub database_url: Option<String>,

    /// Broker backend used for workflow control messages
    #[arg(long, env = "RUNINATOR_BROKER_BACKEND", default_value = "tcp")]
    pub broker_backend: String,

    /// Broker endpoint used for workflow control messages
    #[arg(
        long,
        env = "RUNINATOR_BROKER_ENDPOINT",
        default_value = "127.0.0.1:7070"
    )]
    pub broker_endpoint: String,

    /// How this process reaches the broker: `direct` (the configured backend) or `relay`
    /// (through the authenticated web-service WebSocket endpoint).
    #[arg(long, env = "RUNINATOR_BROKER_MODE", default_value = "direct")]
    pub broker_mode: String,

    /// Web-service base URL used only with `--broker-mode relay`.
    #[arg(long, env = "RUNINATOR_SERVICE_URL")]
    pub service_url: Option<String>,

    /// Bearer credential for the web-service broker relay, used only with `--broker-mode relay`.
    #[arg(long, env = "RUNINATOR_API_KEY")]
    pub api_key: Option<String>,

    /// Relay path relative to `--service-url`; override during a staged endpoint migration.
    #[arg(
        long,
        env = "RUNINATOR_BROKER_RELAY_PATH",
        default_value = DEFAULT_BROKER_RELAY_PATH
    )]
    pub broker_relay_path: String,

    /// Kafka effect topic or RabbitMQ effect queue used by direct broker backends
    #[arg(long, default_value = "runinator.effects")]
    pub broker_effect_topic: String,

    /// Kafka infrastructure-effect topic or RabbitMQ queue used by direct broker backends
    #[arg(long, default_value = "runinator.effects.infrastructure")]
    pub broker_infrastructure_effect_topic: String,

    /// Kafka control topic or RabbitMQ control queue used by direct broker backends
    #[arg(long, default_value = "runinator.control")]
    pub broker_control_topic: String,

    /// Kafka agent topic or RabbitMQ per-replica queue prefix
    #[arg(long, default_value = "runinator.agent")]
    pub broker_agent_topic: String,

    /// Kafka effect-result topic or RabbitMQ effect-result queue used by direct broker backends
    #[arg(long, default_value = "runinator.effect-results")]
    pub broker_effect_result_topic: String,

    /// Kafka wake topic or RabbitMQ wake queue. the engine publishes a timer wake here for every
    /// effect due in the future; it must match the waker's.
    #[arg(long, default_value = "runinator.wake")]
    pub broker_wake_topic: String,

    /// Kafka ingress topic or RabbitMQ ingress queue. the engine is the sole consumer; it must
    /// match the waker's and the worker's.
    #[arg(long, default_value = "runinator.ingress")]
    pub broker_ingress_topic: String,

    /// Kafka/RabbitMQ client id used by direct broker backends
    #[arg(long, default_value = "runinator-engine-worker")]
    pub broker_client_id: String,

    /// Stable instance id used when this worker claims trigger/action-dispatch rows. In Kubernetes this
    /// should be the pod name; otherwise a random per-process id is generated.
    #[arg(long, env = "RUNINATOR_INSTANCE_ID")]
    pub instance_id: Option<String>,

    /// Maximum ingress deliveries the durable engine processes concurrently.
    #[arg(long, env = "RUNINATOR_MAX_CONCURRENT_INGRESS", default_value_t = 16)]
    pub max_concurrent_ingress: usize,
}
