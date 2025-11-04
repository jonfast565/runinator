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
    #[arg(long, default_value = "runinator.db")]
    pub sqlite_path: String,

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
}