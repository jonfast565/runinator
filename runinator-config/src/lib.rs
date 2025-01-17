use clap::Parser;
use runinator_models::errors::SendableError;

#[derive(Parser, Debug, Clone)]
pub struct Config {
    #[clap(long, default_value = "tasks.db")]
    pub database: String,

    #[clap(long, default_value = "3000")]
    pub port: u16,

    #[clap(long, default_value = "./")]
    pub dll_path: String,

    #[clap(long, default_value = "5")]
    pub scheduler_frequency_seconds: u64
}

pub fn parse_config() -> Result<Config, SendableError> {
    let results = Config::try_parse()?;
    Ok(results)
}