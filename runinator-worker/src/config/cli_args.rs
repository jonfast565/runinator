#[allow(unused_imports)]
use super::*;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(super) struct CliArgs {
    /// Show a local full-screen runtime dashboard instead of streaming logs to stdout.
    #[arg(long, env = "RUNINATOR_TUI", default_value_t = false)]
    pub(super) tui: bool,

    #[arg(long = "dll-path")]
    pub(super) dll_paths: Vec<String>,

    /// how to reach the broker: `direct` (the default — connect to `--broker-backend` at
    /// `--broker-endpoint`) or `relay` (tunnel through the web service's `/ws/broker`
    /// endpoint, derived from the service URL). use `relay` for a worker outside the cluster's
    /// trusted network, which only needs outbound access to the web service.
    #[arg(long, env = "RUNINATOR_BROKER_MODE", default_value = "direct")]
    pub(super) broker_mode: String,

    #[arg(long, default_value = "tcp")]
    pub(super) broker_backend: String,

    #[arg(long, default_value = "127.0.0.1:7070")]
    pub(super) broker_endpoint: String,

    #[arg(long, default_value = "runinator.effects")]
    pub(super) broker_effect_topic: String,

    #[arg(long, default_value = "runinator.effects.infrastructure")]
    pub(super) broker_infrastructure_effect_topic: String,

    #[arg(long, default_value = "runinator.control")]
    pub(super) broker_control_topic: String,

    #[arg(long, default_value = "runinator.agent")]
    pub(super) broker_agent_topic: String,

    #[arg(long, default_value = "runinator.effect-results")]
    pub(super) broker_effect_result_topic: String,

    #[arg(long, default_value = "runinator.ingress")]
    pub(super) broker_ingress_topic: String,

    #[arg(long, default_value = "runinator-worker")]
    pub(super) broker_client_id: String,

    #[arg(long)]
    pub(super) broker_consumer_id: Option<String>,

    #[arg(long, default_value_t = 4)]
    pub(super) max_concurrent_actions: usize,

    #[arg(long, default_value_t = 30)]
    pub(super) shutdown_grace_seconds: u64,

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
    pub(super) reconnect_max_attempts: u32,

    #[arg(long, default_value = "http://127.0.0.1:8080/")]
    pub(super) api_base_url: String,

    /// the web service URL, spelled the same way the desktop agent spells it. an alias for
    /// `--api-base-url`; when both are given this one wins.
    #[arg(long, env = "RUNINATOR_SERVICE_URL")]
    pub(super) service_url: Option<String>,

    /// discover a LAN/local-dev service announcement. automatic selection requires an enrollment
    /// token whose cluster id matches the announcement.
    #[arg(long, env = "RUNINATOR_DISCOVER")]
    pub(super) discover: bool,

    #[arg(long, env = "RUNINATOR_GOSSIP_BIND", default_value = "0.0.0.0")]
    pub(super) gossip_bind: String,

    #[arg(long, env = "RUNINATOR_GOSSIP_PORT", default_value_t = 5000)]
    pub(super) gossip_port: u16,

    /// Service API key presented to the web service when auth is enabled.
    #[arg(long, env = "RUNINATOR_API_KEY")]
    pub(super) api_key: Option<String>,

    /// single-use first-start enrollment token. ignored once an issued credential is stored.
    #[arg(long = "enroll", env = "RUNINATOR_ENROLLMENT_TOKEN")]
    pub(super) enrollment_token: Option<String>,

    #[arg(long)]
    pub(super) worker_id: Option<String>,

    // Stable address shown to other components. In Kubernetes, this is the headless-service DNS name,
    // This survives pod IP changes.
    #[arg(long)]
    pub(super) advertise_host: Option<String>,

    /// File touched every 30 seconds for the Kubernetes exec probe.
    /// The worker has no HTTP server. Leave this empty to disable the file.
    #[arg(long, default_value = "/tmp/runinator-worker-liveness")]
    pub(super) liveness_file: String,

    /// comma-separated routing labels this worker advertises, e.g. `runner=desktop,zone=onprem`.
    /// actions that require a label are pinned to a worker carrying it (general pool when empty).
    #[arg(long, env = "RUNINATOR_WORKER_LABELS")]
    pub(super) labels: Option<String>,
}
