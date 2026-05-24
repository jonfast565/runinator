use clap::Parser;

use runinator_models::errors::SendableError;

#[derive(Parser, Debug, Clone)]
pub struct Config {
    #[arg(long, default_value_t = 1)]
    pub scheduler_frequency_seconds: u64,

    #[arg(long, default_value = "0.0.0.0")]
    pub gossip_bind: String,

    #[arg(long, default_value_t = 5000)]
    pub gossip_port: u16,

    #[arg(long, value_delimiter = ',', default_value = "")]
    pub gossip_targets: Vec<String>,

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
    Ok(Config::try_parse()?)
}
