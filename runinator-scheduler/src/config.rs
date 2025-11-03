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
    pub worker_timeout_seconds: u64,

    #[arg(long, default_value_t = 3)]
    pub worker_command_retry: u8,

    #[arg(long, default_value_t = 30)]
    pub api_timeout_seconds: u64,
}

pub fn parse_config() -> Result<Config, SendableError> {
    Ok(Config::try_parse()?)
}
