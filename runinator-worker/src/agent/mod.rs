//! the shared agent runtime.
//!
//! everything a machine needs to join a cluster as a worker replica and stay joined: registration
//! with retry, provider publication, heartbeat, and a supervised action loop that reconnects on its
//! own. the standalone `runinator-worker` binary and `runinator-desktop-agent` both host this, so
//! neither can drift into having a lifecycle behavior the other lacks — the only difference between
//! them is the [`AgentObserver`] attached and the provider set supplied.

pub mod config;
pub mod observer;
pub mod registration;
pub mod relay;
pub mod reporter;
pub mod runtime;
pub mod shutdown;
pub mod status;
pub mod supervisor;

pub use config::{
    AgentRuntimeConfig, BrokerMode, BrokerSelection, DEFAULT_HEARTBEAT_INTERVAL,
    DEFAULT_REGISTER_MAX_ATTEMPTS,
};
pub use observer::{AgentObserver, NoopObserver};
pub use relay::{RELAY_PATH, derive_relay_url};
pub use runtime::{AgentHandle, AgentRuntime};
pub use shutdown::Shutdown;
pub use status::{AgentConnection, AgentMetrics, AgentStatus, CompletedAction, short_id};
