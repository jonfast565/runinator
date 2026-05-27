use clap::Parser;
use uuid::Uuid;

use runinator_models::errors::SendableError;

#[derive(Parser, Debug, Clone)]
pub struct Config {
    #[arg(long, default_value_t = 1)]
    pub scheduler_frequency_seconds: u64,

    #[arg(long, default_value = "")]
    pub scheduler_id: String,

    #[arg(long, default_value_t = 60)]
    pub scheduler_lease_seconds: u64,

    #[arg(long, default_value_t = 50)]
    pub scheduler_claim_limit: i64,

    #[arg(long, default_value = "0.0.0.0")]
    pub gossip_bind: String,

    #[arg(long, default_value_t = 5000)]
    pub gossip_port: u16,

    #[arg(long, value_delimiter = ',', default_value = "")]
    pub gossip_targets: Vec<String>,

    #[arg(long)]
    pub api_base_url: Option<String>,

    #[arg(long, default_value_t = 30)]
    pub api_timeout_seconds: u64,

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

    #[arg(long, default_value = "runinator-scheduler")]
    pub broker_client_id: String,

    #[arg(long, default_value = "disabled")]
    pub worker_control_transport: String,

    #[arg(long, default_value = "127.0.0.1")]
    pub worker_control_bind: String,

    #[arg(long, default_value_t = 7080)]
    pub worker_control_port: u16,
}

pub fn parse_config() -> Result<Config, SendableError> {
    let mut config = Config::try_parse()?;
    if config.scheduler_id.trim().is_empty() {
        config.scheduler_id = format!("scheduler-{}", Uuid::new_v4());
    }
    config.scheduler_claim_limit = config.scheduler_claim_limit.max(1);
    config.scheduler_lease_seconds = config.scheduler_lease_seconds.max(1);
    Ok(config)
}
