use std::path::PathBuf;

use clap::Parser;
use runinator_db_cli::DatabaseBackend;

/// command-line configuration for the standalone background orchestration worker. it mirrors the
/// web service's database and broker options so the same durable engine runs against the same
/// backends, only without the HTTP surface.
#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct CliArgs {
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

    /// Kafka action topic or RabbitMQ action queue used by direct broker backends
    #[arg(long, default_value = "runinator.actions")]
    pub broker_action_topic: String,

    /// Kafka control topic or RabbitMQ control queue used by direct broker backends
    #[arg(long, default_value = "runinator.control")]
    pub broker_control_topic: String,

    /// Kafka result topic or RabbitMQ result queue used by direct broker backends
    #[arg(long, default_value = "runinator.results")]
    pub broker_result_topic: String,

    /// Kafka/RabbitMQ client id used by direct broker backends
    #[arg(long, default_value = "runinator-background-worker")]
    pub broker_client_id: String,

    /// Stable instance id used when this worker claims trigger/action-dispatch rows. In k8s this
    /// should be the pod name; otherwise a random per-process id is generated.
    #[arg(long, env = "RUNINATOR_INSTANCE_ID")]
    pub instance_id: Option<String>,
}
