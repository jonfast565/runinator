use clap::Parser;
use runinator_models::errors::SendableError;
use runinator_utilities::app_data;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Config {
    pub dll_paths: Vec<String>,
    pub broker_backend: String,
    pub broker_endpoint: String,
    pub broker_consumer_id: String,
    pub scheduler_control_transport: String,
    pub scheduler_control_endpoint: String,
    pub max_concurrent_actions: usize,
    pub api_base_url: String,
    pub worker_id: Uuid,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    #[arg(long = "dll-path")]
    dll_paths: Vec<String>,

    #[arg(long, default_value = "tcp")]
    broker_backend: String,

    #[arg(long, default_value = "127.0.0.1:7070")]
    broker_endpoint: String,

    #[arg(long)]
    broker_consumer_id: Option<String>,

    #[arg(long, default_value = "disabled")]
    scheduler_control_transport: String,

    #[arg(long, default_value = "127.0.0.1:7080")]
    scheduler_control_endpoint: String,

    #[arg(long, default_value_t = 4)]
    max_concurrent_actions: usize,

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
        dll_paths: plugin_search_paths(args.dll_paths),
        broker_backend: args.broker_backend,
        broker_endpoint: args.broker_endpoint,
        broker_consumer_id: consumer_id,
        scheduler_control_transport: args.scheduler_control_transport,
        scheduler_control_endpoint: args.scheduler_control_endpoint,
        max_concurrent_actions: args.max_concurrent_actions.max(1),
        api_base_url: args.api_base_url,
        worker_id,
    })
}

fn plugin_search_paths(mut paths: Vec<String>) -> Vec<String> {
    paths.push(default_dll_path());
    paths.sort();
    paths.dedup();
    paths
}

fn default_dll_path() -> String {
    app_data::app_data_path("plugins")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "plugins".to_string())
}
