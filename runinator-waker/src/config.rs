use clap::Parser;
use uuid::Uuid;

use runinator_models::errors::SendableError;

/// the waker is a broker-only timer/relay: it consumes wakes from the web service, sleeps until
/// each is due, then publishes a drive on the ingress channel. it never talks to the web service
/// over http and never shares a channel with the worker.
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

    #[arg(long, default_value = "tcp")]
    pub broker_backend: String,

    #[arg(long, default_value = "127.0.0.1:7070")]
    pub broker_endpoint: String,

    #[arg(long, default_value = "runinator.actions")]
    pub broker_action_topic: String,

    #[arg(long, default_value = "runinator.control")]
    pub broker_control_topic: String,

    #[arg(long, default_value = "runinator.results")]
    pub broker_result_topic: String,

    #[arg(long, default_value = "runinator.wake")]
    pub broker_wake_topic: String,

    #[arg(long, default_value = "runinator.ingress")]
    pub broker_ingress_topic: String,

    #[arg(long, default_value = "runinator-waker")]
    pub broker_client_id: String,

    #[arg(long, default_value = "http://127.0.0.1:8080/")]
    pub api_base_url: String,
}

pub fn parse_config() -> Result<Config, SendableError> {
    let mut config = Config::try_parse()?;
    if config.waker_id.trim().is_empty() {
        config.waker_id = format!("waker-{}", Uuid::new_v4());
    }
    config.max_wake_sleep_seconds = config.max_wake_sleep_seconds.max(1);
    Ok(config)
}
