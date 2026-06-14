use clap::Parser;
use runinator_models::errors::SendableError;
use runinator_utilities::app_data;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Config {
    pub dll_paths: Vec<String>,
    pub broker_backend: String,
    pub broker_endpoint: String,
    pub broker_action_topic: String,
    pub broker_control_topic: String,
    pub broker_result_topic: String,
    pub broker_client_id: String,
    pub broker_consumer_id: String,
    pub max_concurrent_actions: usize,
    pub shutdown_grace_seconds: u64,
    pub api_base_url: String,
    pub api_key: Option<String>,
    pub worker_id: Uuid,
    pub advertise_host: Option<String>,
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

    #[arg(long, default_value = "runinator.actions")]
    broker_action_topic: String,

    #[arg(long, default_value = "runinator.control")]
    broker_control_topic: String,

    #[arg(long, default_value = "runinator.results")]
    broker_result_topic: String,

    #[arg(long, default_value = "runinator-worker")]
    broker_client_id: String,

    #[arg(long)]
    broker_consumer_id: Option<String>,

    #[arg(long, default_value_t = 4)]
    max_concurrent_actions: usize,

    #[arg(long, default_value_t = 30)]
    shutdown_grace_seconds: u64,

    #[arg(long, default_value = "http://127.0.0.1:8080/")]
    api_base_url: String,

    /// Service api key presented to the web service when auth is enabled.
    #[arg(long, env = "RUNINATOR_API_KEY")]
    api_key: Option<String>,

    #[arg(long)]
    worker_id: Option<String>,

    // stable address other components display for this worker; in k8s this is the headless-service
    // dns name so it survives pod ip churn.
    #[arg(long)]
    advertise_host: Option<String>,
}

pub fn parse_config() -> Result<Config, SendableError> {
    let args = CliArgs::parse();
    // a non-uuid identity (e.g. a stable k8s pod name) is folded into a deterministic uuid so the
    // same pod keeps the same replica identity across restarts; a fresh uuid is minted only when no
    // identity is supplied.
    let worker_id = match args.worker_id {
        Some(ref value) if !value.is_empty() => Uuid::parse_str(value)
            .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_DNS, value.as_bytes())),
        _ => Uuid::new_v4(),
    };

    let consumer_id = args.broker_consumer_id.unwrap_or_else(|| {
        if args.broker_backend == "kafka" {
            "runinator-workers".to_string()
        } else {
            worker_id.to_string()
        }
    });

    Ok(Config {
        dll_paths: plugin_search_paths(args.dll_paths),
        broker_backend: args.broker_backend,
        broker_endpoint: args.broker_endpoint,
        broker_action_topic: args.broker_action_topic,
        broker_control_topic: args.broker_control_topic,
        broker_result_topic: args.broker_result_topic,
        broker_client_id: args.broker_client_id,
        broker_consumer_id: consumer_id,
        max_concurrent_actions: args.max_concurrent_actions.max(1),
        shutdown_grace_seconds: args.shutdown_grace_seconds.max(1),
        api_base_url: args.api_base_url,
        api_key: args.api_key.filter(|value| !value.trim().is_empty()),
        worker_id,
        advertise_host: args.advertise_host.filter(|value| !value.trim().is_empty()),
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
