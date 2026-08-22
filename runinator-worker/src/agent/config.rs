//! configuration for the shared agent lifecycle. deliberately host-agnostic: the standalone worker
//! binary builds one of these from its CLI, the desktop agent builds one from its persisted json
//! plus CLI/env overrides, and neither can express a runtime behavior the other cannot.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use runinator_models::errors::SendableError;
use runinator_models::value::Value;

use crate::agent::relay::derive_relay_url;
use crate::broker::BrokerConfig;
use crate::provider_repository::ProviderFactory;

/// how the agent reaches the broker. orthogonal to what kind of worker it is: a cloud worker with no
/// direct path to the broker can relay, and a desktop machine on the trusted network can connect
/// straight to a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrokerMode {
    /// relay through the web service's `/ws/desktop-worker` endpoint, derived from the service URL.
    /// the safe default for a machine that cannot (or should not) reach the broker directly.
    #[default]
    Relay,
    /// connect straight to a broker backend.
    Direct,
}

impl BrokerMode {
    /// the lowercase name, both the serde form and the CLI value.
    pub fn as_str(self) -> &'static str {
        match self {
            BrokerMode::Relay => "relay",
            BrokerMode::Direct => "direct",
        }
    }

    /// parse a CLI/env spelling; `None` when unrecognized so a caller can fall back rather than fail.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "relay" => Some(BrokerMode::Relay),
            "direct" => Some(BrokerMode::Direct),
            _ => None,
        }
    }
}

/// how the service endpoint is chosen before registration. discovery is intentionally explicit;
/// a discovered service is selected automatically only when an enrollment token binds its cluster
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocatorMode {
    #[default]
    Static,
    Discover,
}

/// the inputs that decide which broker a host connects to. resolved into a [`BrokerConfig`] plus a
/// human description of the path taken, which is what both hosts display.
#[derive(Debug, Clone)]
pub struct BrokerSelection {
    pub mode: BrokerMode,
    /// used to derive the relay URL in [`BrokerMode::Relay`]; ignored in `Direct`.
    pub service_url: String,
    /// Backend name (`tcp`/`http`/`rabbitmq`/`kafka`/`in-memory`), used only in `Direct`.
    pub direct_backend: String,
    /// backend endpoint, only used in `Direct`.
    pub direct_endpoint: String,
    pub control_topic: String,
    pub agent_topic: String,
    pub effect_topic: String,
    pub infrastructure_effect_topic: String,
    pub effect_result_topic: String,
    pub client_id: String,
    pub api_key: Option<String>,
}

impl BrokerSelection {
    /// resolve to the broker config to build, and a description such as
    /// `relay via wss://host/ws/desktop-worker` or `direct tcp @ 10.0.0.4:7070`.
    pub fn resolve(self) -> Result<(BrokerConfig, String), SendableError> {
        let (backend, endpoint, description) = match self.mode {
            BrokerMode::Relay => {
                let relay_url = derive_relay_url(&self.service_url)?;
                let description = format!("relay via {relay_url}");
                ("ws".to_string(), relay_url, description)
            }
            BrokerMode::Direct => {
                let description =
                    format!("direct {} @ {}", self.direct_backend, self.direct_endpoint);
                (
                    self.direct_backend.clone(),
                    self.direct_endpoint.clone(),
                    description,
                )
            }
        };

        Ok((
            BrokerConfig {
                broker_backend: backend,
                broker_endpoint: endpoint,
                broker_effect_topic: self.effect_topic,
                broker_infrastructure_effect_topic: self.infrastructure_effect_topic,
                broker_control_topic: self.control_topic,
                broker_agent_topic: self.agent_topic,
                broker_effect_result_topic: self.effect_result_topic,
                broker_client_id: self.client_id,
                api_key: self.api_key,
            },
            description,
        ))
    }
}

/// everything the shared lifecycle needs. built by a host, consumed by
/// [`crate::agent::AgentRuntime::start`].
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
    /// stable identity for this agent; folded into the replica registration so a restart on the
    /// same machine reclaims the same replica row.
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
    /// broker consumer id. `None` uses the replica id, which is what an agent with an identity
    /// assigned at registration time wants; a host that must join a named competing-consumer group
    /// (kafka's `runinator-workers`) supplies it explicitly.
    pub consumer_id: Option<String>,
    /// extra registration attributes, merged with host metadata before registering.
    pub attributes: Value,
    pub broker: BrokerConfig,
    /// human description of the broker path, from [`BrokerSelection::resolve`].
    pub broker_description: String,
    pub providers: ProviderFactory,
    /// publish each provider's metadata to the web service after registering. the desktop agent
    /// does; the standalone worker binary does not, because in-cluster provider metadata is
    /// published by whichever worker registers first and the extra round trips buy nothing.
    pub publish_providers: bool,
    /// filesystem search paths for dynamic plugins. host-only: a container image links statically
    /// and has no dynamic loader, so this is empty there.
    pub dll_paths: Vec<String>,
    pub max_concurrent_actions: usize,
    pub shutdown_grace: Duration,
    /// path touched on an interval to signal liveness; empty disables the probe.
    pub liveness_file: String,
    pub heartbeat_interval: Duration,
    /// advertised health window; remote agents use a wider window than in-cluster workers.
    pub stale_after: Duration,
    /// how many times to retry replica registration before giving up. registration is interruptible
    /// by shutdown regardless.
    pub register_max_attempts: u32,
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

/// default replica heartbeat cadence, matching what both hosts used before they shared this config.
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

/// default registration retry budget; roughly two minutes of backoff before giving up.
pub const DEFAULT_REGISTER_MAX_ATTEMPTS: u32 = 8;

/// default reconnect budget for a host that wants a finite one: ten consecutive failures, which the
/// capped backoff spreads over roughly seven minutes before the agent stops.
pub const DEFAULT_RECONNECT_MAX_ATTEMPTS: u32 = 10;

/// retry indefinitely, for a host whose orchestrator restarts it on exit.
pub const RECONNECT_UNLIMITED: u32 = 0;

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
