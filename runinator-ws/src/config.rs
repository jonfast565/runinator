use std::path::PathBuf;

use clap::Parser;
use runinator_db_cli::DatabaseBackend;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct CliArgs {
    /// Webservice port to bind to, defaults to 8080
    #[arg(long, env = "RUNINATOR_PORT", default_value_t = 8080)]
    pub port: u16,

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

    /// Address to bind the gossip socket for service discovery
    #[arg(long, default_value = "0.0.0.0")]
    pub gossip_bind: String,

    /// Gossip UDP port
    #[arg(long, default_value_t = 5000)]
    pub gossip_port: u16,

    /// Additional gossip targets as host:port, comma separated
    #[arg(long, value_delimiter = ',', default_value = "")]
    pub gossip_targets: Vec<String>,

    /// Disable gossip advertisements for environments with deterministic service DNS
    #[arg(long)]
    pub disable_gossip: bool,

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

    /// Stable address advertised to the replica list; in k8s this is the pod's headless-service DNS
    /// name so it stays resolvable across IP churn.
    #[arg(long, default_value = "")]
    pub advertise_host: String,

    /// Stable instance id advertised to the replica list. In k8s this should be the pod name.
    #[arg(long, env = "RUNINATOR_INSTANCE_ID")]
    pub instance_id: Option<String>,

    /// Require authentication on the HTTP API. Off by default so the local/dev stack runs unchanged.
    #[arg(long, env = "RUNINATOR_AUTH_ENABLED", default_value_t = false)]
    pub auth_enabled: bool,

    /// Access-token lifetime in seconds (default 1 hour).
    #[arg(
        long,
        env = "RUNINATOR_AUTH_ACCESS_TTL_SECONDS",
        default_value_t = 3600
    )]
    pub auth_access_ttl_seconds: i64,

    /// Refresh-token lifetime in seconds (default 14 days).
    #[arg(
        long,
        env = "RUNINATOR_AUTH_REFRESH_TTL_SECONDS",
        default_value_t = 1_209_600
    )]
    pub auth_refresh_ttl_seconds: i64,

    /// Enable per-principal/per-IP rate limiting on the HTTP API. On by default; set to false to
    /// disable. The unauthenticated auth endpoints carry a separate, always-on brute-force throttle.
    #[arg(long, env = "RUNINATOR_RATE_LIMIT_ENABLED", default_value_t = true)]
    pub rate_limit_enabled: bool,

    /// Sustained requests per second allowed per principal/IP (token-bucket refill rate).
    #[arg(long, env = "RUNINATOR_RATE_LIMIT_RPS", default_value_t = 50.0)]
    pub rate_limit_rps: f64,

    /// Maximum burst capacity per principal/IP (token-bucket size).
    #[arg(long, env = "RUNINATOR_RATE_LIMIT_BURST", default_value_t = 100.0)]
    pub rate_limit_burst: f64,

    /// Enable global overload protection (a concurrency cap + per-request timeout) on the HTTP API.
    /// On by default; set to false to disable both the concurrency limit and the request timeout.
    #[arg(
        long,
        env = "RUNINATOR_OVERLOAD_PROTECTION_ENABLED",
        default_value_t = true
    )]
    pub overload_protection_enabled: bool,

    /// Maximum HTTP requests processed concurrently before excess load is shed with 503.
    #[arg(long, env = "RUNINATOR_MAX_CONCURRENT_REQUESTS", default_value_t = 512)]
    pub max_concurrent_requests: usize,

    /// Per-request wall-clock timeout in seconds; a slower handler is aborted with 408.
    #[arg(long, env = "RUNINATOR_REQUEST_TIMEOUT_SECONDS", default_value_t = 30)]
    pub request_timeout_seconds: u64,

    /// Run the durable orchestration engine (reducer, wake/trigger/action/ingress loops, result
    /// consumer, maintenance backstops) in-process. On by default so the single-process local/dev
    /// stack runs unchanged; set to false when a standalone `runinator-background-worker` owns the
    /// engine and this replica serves HTTP/WebSocket only.
    #[arg(long, env = "RUNINATOR_WS_RUN_ENGINE", default_value_t = true)]
    pub run_engine: bool,
}
