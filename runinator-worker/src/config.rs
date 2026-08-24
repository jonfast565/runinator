use clap::Parser;
use runinator_models::errors::SendableError;
use runinator_platform::app_data;
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

use crate::agent::{
    AgentRuntimeConfig, BrokerMode, BrokerSelection, DEFAULT_HEARTBEAT_INTERVAL, LocatorMode,
    RECONNECT_UNLIMITED,
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
    pub broker_control_topic: String,
    pub broker_agent_topic: String,
    pub broker_effect_topic: String,
    pub broker_infrastructure_effect_topic: String,
    pub broker_effect_result_topic: String,
    pub broker_ingress_topic: String,
    pub broker_client_id: String,
    pub broker_consumer_id: String,
    pub max_concurrent_actions: usize,
    pub shutdown_grace_seconds: u64,
    /// consecutive reconnect failures tolerated before the worker stops itself; `0` retries forever.
    pub reconnect_max_attempts: u32,
    pub api_base_url: String,
    pub locator_mode: LocatorMode,
    pub gossip_bind: String,
    pub gossip_port: u16,
    pub api_key: Option<String>,
    pub enrollment_token: Option<String>,
    pub worker_id: Uuid,
    pub advertise_host: Option<String>,
    pub liveness_file: String,
    /// routing labels this worker advertises; the engine pins label-targeted effects to a worker
    /// whose labels are a superset of the action's required selector.
    pub labels: BTreeMap<String, String>,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    #[arg(long = "dll-path")]
    dll_paths: Vec<String>,

    /// how to reach the broker: `direct` (the default — connect to `--broker-backend` at
    /// `--broker-endpoint`) or `relay` (tunnel through the web service's `/ws/broker`
    /// endpoint, derived from the service URL). use `relay` for a worker outside the cluster's
    /// trusted network, which only needs outbound access to the web service.
    #[arg(long, env = "RUNINATOR_BROKER_MODE", default_value = "direct")]
    broker_mode: String,

    #[arg(long, default_value = "tcp")]
    broker_backend: String,

    #[arg(long, default_value = "127.0.0.1:7070")]
    broker_endpoint: String,

    #[arg(long, default_value = "runinator.effects")]
    broker_effect_topic: String,

    #[arg(long, default_value = "runinator.effects.infrastructure")]
    broker_infrastructure_effect_topic: String,

    #[arg(long, default_value = "runinator.control")]
    broker_control_topic: String,

    #[arg(long, default_value = "runinator.agent")]
    broker_agent_topic: String,

    #[arg(long, default_value = "runinator.effect-results")]
    broker_effect_result_topic: String,

    #[arg(long, default_value = "runinator.ingress")]
    broker_ingress_topic: String,

    #[arg(long, default_value = "runinator-worker")]
    broker_client_id: String,

    #[arg(long)]
    broker_consumer_id: Option<String>,

    #[arg(long, default_value_t = 4)]
    max_concurrent_actions: usize,

    #[arg(long, default_value_t = 30)]
    shutdown_grace_seconds: u64,

    /// how many consecutive failed reconnects to tolerate before the worker disconnects and exits
    /// non-zero. the count resets after an attempt that stays up, so this bounds one outage rather
    /// than the process's lifetime. defaults to `0` — retry forever — because an in-cluster worker's
    /// orchestrator is what decides whether a pod that cannot reach the broker should be restarted
    /// or rescheduled.
    #[arg(
        long,
        env = "RUNINATOR_RECONNECT_MAX_ATTEMPTS",
        default_value_t = RECONNECT_UNLIMITED
    )]
    reconnect_max_attempts: u32,

    #[arg(long, default_value = "http://127.0.0.1:8080/")]
    api_base_url: String,

    /// the web service URL, spelled the same way the desktop agent spells it. an alias for
    /// `--api-base-url`; when both are given this one wins.
    #[arg(long, env = "RUNINATOR_SERVICE_URL")]
    service_url: Option<String>,

    /// discover a LAN/local-dev service announcement. automatic selection requires an enrollment
    /// token whose cluster id matches the announcement.
    #[arg(long, env = "RUNINATOR_DISCOVER")]
    discover: bool,

    #[arg(long, env = "RUNINATOR_GOSSIP_BIND", default_value = "0.0.0.0")]
    gossip_bind: String,

    #[arg(long, env = "RUNINATOR_GOSSIP_PORT", default_value_t = 5000)]
    gossip_port: u16,

    /// Service API key presented to the web service when auth is enabled.
    #[arg(long, env = "RUNINATOR_API_KEY")]
    api_key: Option<String>,

    /// single-use first-start enrollment token. ignored once an issued credential is stored.
    #[arg(long = "enroll", env = "RUNINATOR_ENROLLMENT_TOKEN")]
    enrollment_token: Option<String>,

    #[arg(long)]
    worker_id: Option<String>,

    // Stable address shown to other components. In Kubernetes, this is the headless-service DNS name,
    // This survives pod IP changes.
    #[arg(long)]
    advertise_host: Option<String>,

    /// File touched every 30 seconds for the Kubernetes exec probe.
    /// The worker has no HTTP server. Leave this empty to disable the file.
    #[arg(long, default_value = "/tmp/runinator-worker-liveness")]
    liveness_file: String,

    /// comma-separated routing labels this worker advertises, e.g. `runner=creds-sync,zone=onprem`.
    /// actions that require a label are pinned to a worker carrying it (general pool when empty).
    #[arg(long, env = "RUNINATOR_WORKER_LABELS")]
    labels: Option<String>,
}

pub fn parse_config() -> Result<Config, SendableError> {
    let args = CliArgs::parse();
    // A non-UUID identity, such as a stable Kubernetes pod name, is folded into a deterministic UUID.
    // The same pod keeps the same replica identity across restarts; a fresh UUID is minted only when no
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
        broker_control_topic: args.broker_control_topic,
        broker_agent_topic: args.broker_agent_topic,
        broker_effect_topic: args.broker_effect_topic,
        broker_infrastructure_effect_topic: args.broker_infrastructure_effect_topic,
        broker_effect_result_topic: args.broker_effect_result_topic,
        broker_ingress_topic: args.broker_ingress_topic,
        broker_client_id: args.broker_client_id,
        broker_consumer_id: consumer_id,
        max_concurrent_actions: args.max_concurrent_actions.max(1),
        shutdown_grace_seconds: args.shutdown_grace_seconds.max(1),
        reconnect_max_attempts: args.reconnect_max_attempts,
        api_base_url,
        locator_mode: if args.discover {
            LocatorMode::Discover
        } else {
            LocatorMode::Static
        },
        gossip_bind: args.gossip_bind,
        gossip_port: args.gossip_port,
        api_key: args.api_key.filter(|value| !value.trim().is_empty()),
        enrollment_token: args
            .enrollment_token
            .filter(|value| !value.trim().is_empty()),
        worker_id,
        advertise_host: args.advertise_host.filter(|value| !value.trim().is_empty()),
        liveness_file: args.liveness_file,
        labels: parse_labels(args.labels.as_deref()),
    })
}

impl Config {
    /// map this worker's CLI config onto the shared agent lifecycle.
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
            control_topic: self.broker_control_topic.clone(),
            agent_topic: self.broker_agent_topic.clone(),
            effect_topic: self.broker_effect_topic.clone(),
            infrastructure_effect_topic: self.broker_infrastructure_effect_topic.clone(),
            effect_result_topic: self.broker_effect_result_topic.clone(),
            ingress_topic: self.broker_ingress_topic.clone(),
            client_id: self.broker_client_id.clone(),
            api_key: self.api_key.clone(),
        }
        .resolve()?;

        Ok(AgentRuntimeConfig {
            service_url: self.api_base_url.clone(),
            locator_mode: self.locator_mode,
            gossip_bind: self.gossip_bind.clone(),
            gossip_port: self.gossip_port,
            api_key: self.api_key.clone(),
            enrollment_token: self.enrollment_token.clone(),
            credential_file: app_data::app_data_path("agent/worker-credential.json")
                .unwrap_or_else(|_| std::path::PathBuf::from("worker-credential.json")),
            outbox_file: app_data::app_data_path("agent/worker-result-outbox.jsonl")
                .unwrap_or_else(|_| std::path::PathBuf::from("worker-result-outbox.jsonl")),
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
            stale_after: Duration::from_secs(30),
            reconnect_max_attempts: self.reconnect_max_attempts,
            sample_telemetry: true,
            directive_handler: std::sync::Arc::new(crate::agent::DefaultDirectiveHandler),
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
