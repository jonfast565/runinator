//! configuration for the shared agent lifecycle. deliberately host-agnostic: the standalone worker
//! binary builds one of these from its CLI, the desktop agent builds one from its persisted json
//! plus CLI/env overrides, and neither can express a runtime behavior the other cannot.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use runinator_broker::{BrokerClientConfig, select_broker_connection};
use runinator_models::errors::SendableError;
use runinator_models::value::Value;

use crate::broker::BrokerConfig;
use crate::provider_repository::ProviderFactory;

/// how the agent reaches the broker. orthogonal to what kind of worker it is: a cloud worker with no
/// direct path to the broker can relay, and a desktop machine on the trusted network can connect
/// straight to a backend.
pub use runinator_broker::BrokerConnectionMode as BrokerMode;

/// how the service endpoint is chosen before registration. discovery is intentionally explicit;
/// a discovered service is selected automatically only when an enrollment token binds its cluster
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocatorMode {
    #[default]
    Static,
    Discover,
}

/// everything the shared lifecycle needs. built by a host, consumed by
/// [`crate::agent::AgentRuntime::start`].

/// default replica heartbeat cadence, matching what both hosts used before they shared this config.
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

/// default reconnect budget for a host that wants a finite one: ten consecutive failures, which the
/// capped backoff spreads over roughly seven minutes before the agent stops.
pub const DEFAULT_RECONNECT_MAX_ATTEMPTS: u32 = 10;

/// retry indefinitely, for a host whose orchestrator restarts it on exit.
pub const RECONNECT_UNLIMITED: u32 = 0;

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

mod broker_selection;
pub use broker_selection::BrokerSelection;

mod agent_runtime_config;
pub use agent_runtime_config::AgentRuntimeConfig;
