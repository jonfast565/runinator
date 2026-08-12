use clap::Parser;
use runinator_models::errors::SendableError;
use runinator_utilities::app_data;
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

use crate::agent::{
    AgentRuntimeConfig, BrokerMode, BrokerSelection, DEFAULT_HEARTBEAT_INTERVAL,
    DEFAULT_REGISTER_MAX_ATTEMPTS,
};
use crate::provider_repository::default_provider_factory;

#[derive(Debug, Clone)]
pub struct Config {
    pub dll_paths: Vec<String>,
    /// how this worker reaches the broker. `Direct` uses `broker_backend`/`broker_endpoint` as
    /// given; `Relay` ignores both and tunnels through the web service instead.
    pub broker_mode: BrokerMode,
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
    pub liveness_file: String,
    /// routing labels this worker advertises; the reducer pins label-targeted actions to a worker
    /// whose labels are a superset of the action's required selector.
    pub labels: BTreeMap<String, String>,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    #[arg(long = "dll-path")]
    dll_paths: Vec<String>,

    /// how to reach the broker: `direct` (the default — connect to `--broker-backend` at
    /// `--broker-endpoint`) or `relay` (tunnel through the web service's `/ws/desktop-worker`
    /// endpoint, derived from the service url). use `relay` for a worker outside the cluster's
    /// trusted network, which only needs outbound access to the web service.
    #[arg(long, env = "RUNINATOR_BROKER_MODE", default_value = "direct")]
    broker_mode: String,

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

    /// the web service url, spelled the same way the desktop agent spells it. an alias for
    /// `--api-base-url`; when both are given this one wins.
    #[arg(long, env = "RUNINATOR_SERVICE_URL")]
    service_url: Option<String>,

    /// Service api key presented to the web service when auth is enabled.
    #[arg(long, env = "RUNINATOR_API_KEY")]
    api_key: Option<String>,

    #[arg(long)]
    worker_id: Option<String>,

    // stable address other components display for this worker; in k8s this is the headless-service
    // dns name so it survives pod ip churn.
    #[arg(long)]
    advertise_host: Option<String>,

    /// path to a file that is touched every 30 seconds to signal liveness; used with k8s exec
    /// probes when the worker has no http server. set to empty to disable.
    #[arg(long, default_value = "/tmp/runinator-worker-liveness")]
    liveness_file: String,

    /// comma-separated routing labels this worker advertises, e.g. `runner=creds-sync,zone=onprem`.
    /// actions that require a label are pinned to a worker carrying it (general pool when empty).
    #[arg(long, env = "RUNINATOR_WORKER_LABELS")]
    labels: Option<String>,
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

    let broker_mode = BrokerMode::parse(&args.broker_mode).ok_or_else(|| {
        crate::errors::BROKER_UNKNOWN_BACKEND
            .error(format!("unknown --broker-mode '{}'", args.broker_mode))
    })?;
    let api_base_url = args
        .service_url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(args.api_base_url);

    Ok(Config {
        dll_paths: plugin_search_paths(args.dll_paths),
        broker_mode,
        broker_backend: args.broker_backend,
        broker_endpoint: args.broker_endpoint,
        broker_action_topic: args.broker_action_topic,
        broker_control_topic: args.broker_control_topic,
        broker_result_topic: args.broker_result_topic,
        broker_client_id: args.broker_client_id,
        broker_consumer_id: consumer_id,
        max_concurrent_actions: args.max_concurrent_actions.max(1),
        shutdown_grace_seconds: args.shutdown_grace_seconds.max(1),
        api_base_url,
        api_key: args.api_key.filter(|value| !value.trim().is_empty()),
        worker_id,
        advertise_host: args.advertise_host.filter(|value| !value.trim().is_empty()),
        liveness_file: args.liveness_file,
        labels: parse_labels(args.labels.as_deref()),
    })
}

impl Config {
    /// map this worker's cli config onto the shared agent lifecycle.
    ///
    /// the worker is a general-pool, non-exclusive replica: it takes untargeted work, keeps the
    /// explicitly configured broker consumer id (kafka's shared group depends on it), and does not
    /// publish provider metadata — an in-cluster deployment has many identical workers and the extra
    /// round trips buy nothing.
    pub fn agent_runtime_config(&self) -> Result<AgentRuntimeConfig, SendableError> {
        let (broker, broker_description) = BrokerSelection {
            mode: self.broker_mode,
            service_url: self.api_base_url.clone(),
            direct_backend: self.broker_backend.clone(),
            direct_endpoint: self.broker_endpoint.clone(),
            action_topic: self.broker_action_topic.clone(),
            control_topic: self.broker_control_topic.clone(),
            result_topic: self.broker_result_topic.clone(),
            client_id: self.broker_client_id.clone(),
            api_key: self.api_key.clone(),
        }
        .resolve()?;

        Ok(AgentRuntimeConfig {
            service_url: self.api_base_url.clone(),
            api_key: self.api_key.clone(),
            instance_id: self.worker_id.to_string(),
            display_name: Some(format!("worker-{}", self.worker_id)),
            advertise_host: self.advertise_host.clone(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            labels: self.labels.clone(),
            exclusive: false,
            consumer_id: Some(self.broker_consumer_id.clone()),
            attributes: runinator_models::json!({
                "broker_client_id": self.broker_client_id,
                "broker_consumer_id": self.broker_consumer_id,
            }),
            broker,
            broker_description,
            providers: default_provider_factory(),
            publish_providers: false,
            dll_paths: self.dll_paths.clone(),
            max_concurrent_actions: self.max_concurrent_actions,
            shutdown_grace: Duration::from_secs(self.shutdown_grace_seconds),
            liveness_file: self.liveness_file.clone(),
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            register_max_attempts: DEFAULT_REGISTER_MAX_ATTEMPTS,
            sample_telemetry: true,
        })
    }
}

/// parse a `k=v,k=v` label string into a map; blank entries and entries without a `=` are skipped.
/// shared with `runinator-desktop-agent` so both surfaces accept the same label syntax.
pub fn parse_labels(raw: Option<&str>) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    let Some(raw) = raw else {
        return labels;
    };
    for entry in raw.split(',') {
        let Some((key, value)) = entry.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        labels.insert(key.to_string(), value.to_string());
    }
    labels
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
