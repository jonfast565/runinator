use clap::Parser;
use runinator_broker::DEFAULT_BROKER_RELAY_PATH;
use uuid::Uuid;

use runinator_models::errors::SendableError;

/// the waker is a broker-only timer/relay: it consumes wakes from the engine, sleeps until each is
/// due, then publishes the wake's carried effect settle on the ingress channel. It has no web API
/// dependency: when it runs outside the broker network, its broker calls travel through the
/// authenticated WebSocket relay instead.
#[derive(Parser, Debug, Clone)]
pub struct Config {
    /// Show a local full-screen runtime dashboard instead of streaming logs to stdout.
    #[arg(long, env = "RUNINATOR_TUI", default_value_t = false)]
    pub tui: bool,

    /// Stable process identity shown in the replica list. A generated value is sufficient when the
    /// host has no durable identity; orchestrators should pass the pod or service instance name.
    #[arg(long, default_value = "")]
    pub waker_id: String,

    /// consumer group shared across waker replicas so a wake is handled by exactly one of them.
    #[arg(long, default_value = "runinator-waker")]
    pub waker_consumer_group: String,

    /// upper bound on a single sleep before a not-yet-due wake is returned to the broker for
    /// re-evaluation. keep below the broker visibility lease (30s for the in-memory backend).
    #[arg(long, default_value_t = 20)]
    pub max_wake_sleep_seconds: u64,

    /// wakes handled concurrently by this replica, so a not-yet-due wake sleeping toward its due
    /// time never head-of-line blocks a due wake behind it.
    #[arg(long, default_value_t = 32)]
    pub max_concurrent_wakes: usize,

    #[arg(long, default_value = "tcp")]
    pub broker_backend: String,

    #[arg(long, default_value = "127.0.0.1:7070")]
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

    #[arg(long, default_value = "runinator-waker")]
    pub broker_client_id: String,

    /// Address displayed for this broker-only replica. It is descriptive only; nothing dials a
    /// waker directly.
    #[arg(long, default_value = "")]
    pub advertise_host: String,

    /// Cadence for the broker health heartbeat. It verifies the transport while the wake consumer
    /// is idle, without sending a durable message to any workflow channel.
    #[arg(long, default_value_t = 10)]
    pub broker_heartbeat_seconds: u64,

    /// File touched every 30 seconds for the Kubernetes exec probe.
    /// The waker has no HTTP server. Leave this empty to disable the file.
    #[arg(long, default_value = "/tmp/runinator-waker-liveness")]
    pub liveness_file: String,
}

pub fn parse_config() -> Result<Config, SendableError> {
    Ok(normalize_config(Config::try_parse()?))
}

pub fn normalize_config(mut config: Config) -> Config {
    if config.waker_id.trim().is_empty() {
        config.waker_id = format!("waker-{}", Uuid::now_v7());
    }
    config.max_wake_sleep_seconds = config.max_wake_sleep_seconds.max(1);
    config.max_concurrent_wakes = config.max_concurrent_wakes.max(1);
    config.broker_heartbeat_seconds = config.broker_heartbeat_seconds.max(1);
    config
}
