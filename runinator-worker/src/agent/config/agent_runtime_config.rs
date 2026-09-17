#[allow(unused_imports)]
use super::*;

pub struct AgentRuntimeConfig {
    /// web service base URL; the API client and (in relay mode) the broker endpoint come from it.
    pub service_url: String,
    pub locator_mode: LocatorMode,
    pub gossip_bind: String,
    pub gossip_port: u16,
    pub api_key: Option<String>,
    /// one-time token supplied only for first start; never persisted by the runtime.
    pub enrollment_token: Option<String>,
    /// owner-only file holding the issued key and stable identity after redemption.
    pub credential_file: PathBuf,
    /// fsynced terminal-result buffer, adjacent to the issued credential under app data.
    pub outbox_file: PathBuf,
    /// Stable instance identity for this agent. Each runtime activation mints a fresh replica id,
    /// while this value links successive activations in the fleet history.
    pub instance_id: String,
    pub display_name: Option<String>,
    /// stable address other components display for this agent, when it has one.
    pub advertise_host: Option<String>,
    pub version: Option<String>,
    /// routing labels this replica advertises; a label-targeted action only lands here when these
    /// satisfy its selector.
    pub labels: BTreeMap<String, String>,
    /// when true the consumer never picks up general-pool `Any` work — only actions pinned to this
    /// replica id or targeted at a label it advertises.
    pub exclusive: bool,
    /// broker consumer id. `None` uses the preallocated replica id; a host that must join a named competing-consumer group
    /// (kafka's `runinator-workers`) supplies it explicitly.
    pub consumer_id: Option<String>,
    /// extra registration attributes, merged with host metadata before registering.
    pub attributes: Value,
    pub broker: BrokerConfig,
    /// human description of the broker path, from [`BrokerSelection::resolve`].
    pub broker_description: String,
    pub providers: ProviderFactory,
    /// publish each provider's metadata with the broker availability announcement. the desktop agent
    /// does; the standalone worker binary does not, because in-cluster provider metadata is
    /// published by whichever worker registers first and the extra round trips buy nothing.
    pub publish_providers: bool,
    /// filesystem search paths for dynamic plugins. host-only: a container image links statically
    /// and has no dynamic loader, so this is empty there.
    pub dll_paths: Vec<String>,
    pub max_concurrent_actions: usize,
    pub shutdown_grace: Duration,
    /// when true, replace these process defaults with the persisted platform worker policy and
    /// refresh it while the runtime is active. desktop agents keep their machine-local policy.
    pub use_server_worker_settings: bool,
    pub worker_settings_refresh_interval: Duration,
    /// path touched on an interval to signal liveness; empty disables the probe.
    pub liveness_file: String,
    pub heartbeat_interval: Duration,
    /// advertised health window; remote agents use a wider window than in-cluster workers.
    pub stale_after: Duration,
    /// how many *consecutive* failed worker-loop attempts to tolerate before the agent gives up and
    /// stops itself. the counter resets once an attempt stays up long enough to call it healthy, so
    /// this bounds one unreachable episode rather than a machine's lifetime. `0` retries forever,
    /// which is what an orchestrator-supervised worker wants; a desktop agent nobody is watching
    /// wants a finite budget so it drops off the registry instead of spinning against a dead broker.
    pub reconnect_max_attempts: u32,
    /// sample host cpu/memory on every heartbeat, so this agent reports the same telemetry an
    /// in-cluster worker does.
    pub sample_telemetry: bool,
    /// host-specific implementation for bounded log and sandbox reads.
    pub directive_handler: std::sync::Arc<dyn crate::agent::directives::DirectiveHandler>,
}
