use clap::Parser;
use runinator_models::errors::SendableError;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Config {
    pub dll_path: String,
    pub broker_backend: String,
    pub broker_endpoint: String,
    pub broker_consumer_id: String,
    pub broker_poll_timeout_seconds: u64,
    pub api_base_url: String,
    pub worker_id: Uuid,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    #[arg(long, default_value = "/opt/runinator/plugins")]
    dll_path: String,

    #[arg(long, default_value = "http")]
    broker_backend: String,

    #[arg(long, default_value = "http://127.0.0.1:7070/")]
    broker_endpoint: String,

    #[arg(long)]
    broker_consumer_id: Option<String>,

    #[arg(long, default_value_t = 5)]
    broker_poll_timeout_seconds: u64,

    #[arg(long, default_value = "http://127.0.0.1:8080/")]
    api_base_url: String,

    #[arg(long)]
    worker_id: Option<String>,
}

pub fn parse_config() -> Result<Config, SendableError> {
    let args = CliArgs::parse();
    let worker_id = match args.worker_id {
        Some(ref value) if !value.is_empty() => {
            Uuid::parse_str(value).map_err(|err| -> SendableError { Box::new(err) })?
        }
        _ => Uuid::new_v4(),
    };

    let consumer_id = args
        .broker_consumer_id
        .unwrap_or_else(|| worker_id.to_string());

    Ok(Config {
        dll_path: args.dll_path,
        broker_backend: args.broker_backend,
        broker_endpoint: args.broker_endpoint,
        broker_consumer_id: consumer_id,
        broker_poll_timeout_seconds: args.broker_poll_timeout_seconds,
        api_base_url: args.api_base_url,
        worker_id,
    })
}
