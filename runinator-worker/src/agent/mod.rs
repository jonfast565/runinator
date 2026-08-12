//! the shared agent runtime.
//!
//! everything a machine needs to join a cluster as a worker replica and stay joined: registration
//! with retry, provider publication, heartbeat, and a supervised action loop that reconnects on its
//! own. the standalone `runinator-worker` binary and `runinator-desktop-agent` both host this, so
//! neither can drift into having a lifecycle behavior the other lacks — the only difference between
//! them is the [`AgentObserver`] attached and the provider set supplied.

pub mod config;
pub mod directives;
pub mod enroll;
pub mod observer;
pub mod outbox;
pub mod registration;
pub mod relay;
pub mod reporter;
pub mod runtime;
pub mod shutdown;
pub mod status;
pub mod supervisor;

pub use config::{
    AgentRuntimeConfig, BrokerMode, BrokerSelection, DEFAULT_HEARTBEAT_INTERVAL,
    DEFAULT_REGISTER_MAX_ATTEMPTS, LocatorMode,
};
pub use directives::{DefaultDirectiveHandler, DirectiveHandler, DirectiveResponse};
pub use enroll::prepare_agent_credentials;
pub use observer::{AgentObserver, NoopObserver};
pub use outbox::{FileOutbox, NoopOutbox, ResultOutbox};
pub use relay::{RELAY_PATH, derive_relay_url, derive_relay_url_with_path};
pub use runtime::{AgentHandle, AgentRuntime};
pub use shutdown::Shutdown;
pub use status::{AgentConnection, AgentMetrics, AgentStatus, CompletedAction, short_id};
