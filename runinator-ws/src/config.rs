use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum DatabaseKind {
    Sqlite,
    Postgres,
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct CliArgs {
    /// Webservice port to bind to, defaults to 8080
    #[arg(long, default_value_t = 8080)]
    pub port: u16,

    /// Database backend to use
    #[arg(long, value_enum, default_value_t = DatabaseKind::Sqlite)]
    pub database: DatabaseKind,

    /// Path to the SQLite database file (used when --database=sqlite)
    #[arg(long)]
    pub sqlite_path: Option<PathBuf>,

    /// Connection string for the database (required when --database=postgres)
    #[arg(long)]
    pub database_url: Option<String>,

    /// Address to bind the gossip socket for service discovery
    #[arg(long, default_value = "0.0.0.0")]
    pub gossip_bind: String,

    /// Gossip UDP port
    #[arg(long, default_value_t = 5000)]
    pub gossip_port: u16,

    /// Additional gossip targets as host:port, comma separated
    #[arg(long, value_delimiter = ',', default_value = "")]
    pub gossip_targets: Vec<String>,

    /// Address advertised to other services (e.g. public IP or pod IP)
    #[arg(long, default_value = "127.0.0.1")]
    pub announce_address: String,

    /// Base path advertised to other services
    #[arg(long, default_value = "/")]
    pub announce_base_path: String,

    /// Seconds between gossip announcements
    #[arg(long, default_value_t = 5)]
    pub gossip_interval_seconds: u64,

    /// Broker backend used for workflow control messages
    #[arg(long, default_value = "tcp")]
    pub broker_backend: String,

    /// Broker endpoint used for workflow control messages
    #[arg(long, default_value = "127.0.0.1:7070")]
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
    #[arg(long, default_value = "runinator-ws")]
    pub broker_client_id: String,
}
