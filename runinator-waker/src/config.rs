use clap::Parser;
use uuid::Uuid;

use runinator_models::errors::SendableError;

/// the waker is a broker-only timer/relay: it consumes wakes from the engine, sleeps until each is
/// due, then publishes the wake's carried effect settle on the ingress channel. it never talks to
/// the web service over http and never shares a channel with the worker.
#[derive(Parser, Debug, Clone)]
pub struct Config {
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

    #[arg(long, default_value = "http://127.0.0.1:8080/")]
    pub api_base_url: String,

    /// service API key presented to the web service when auth is enabled.
    #[arg(long, env = "RUNINATOR_API_KEY")]
    pub api_key: Option<String>,

    /// Stable address advertised to the replica list. In Kubernetes, use the pod's headless-service
    /// DNS name so it stays resolvable when the pod IP changes.
    #[arg(long, default_value = "")]
    pub advertise_host: String,

    /// File touched every 30 seconds for the Kubernetes exec probe.
    /// The waker has no HTTP server. Leave this empty to disable the file.
    #[arg(long, default_value = "/tmp/runinator-waker-liveness")]
    pub liveness_file: String,
}

pub fn parse_config() -> Result<Config, SendableError> {
    let mut config = Config::try_parse()?;
    if config.waker_id.trim().is_empty() {
        config.waker_id = format!("waker-{}", Uuid::new_v4());
    }
    config.max_wake_sleep_seconds = config.max_wake_sleep_seconds.max(1);
    config.max_concurrent_wakes = config.max_concurrent_wakes.max(1);
    Ok(config)
}
