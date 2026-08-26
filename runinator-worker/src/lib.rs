//! the worker runtime: an action/control loop that resolves providers and executes task nodes,
//! publishing results back through the broker. exposed as a library so the standalone binary and an
//! embedded host (the desktop command center) can both drive the same loop.

pub mod agent;
pub mod artifact_upload;
pub mod broker;
pub mod config;
mod effect_worker;
pub mod errors;
pub mod events;
pub mod executor;
mod file_inputs;
pub mod function_cache;
pub mod metrics;
pub mod provider_repository;
pub mod secrets;
pub mod worker;

pub use agent::{
    AgentConnection, AgentHandle, AgentMetrics, AgentObserver, AgentRuntime, AgentRuntimeConfig,
    AgentStatus, BrokerMode, BrokerSelection, CompletedAction, FileOutbox, NoopObserver,
    NoopOutbox, ResultOutbox, derive_relay_url, prepare_agent_credentials,
};
pub use broker::{BrokerConfig, build_broker};
pub use config::{Config, parse_config, parse_labels};
pub use events::{ActionOutcome, NoopEventSink, WorkerEvent, WorkerEventSink};
pub use provider_repository::{ProviderFactory, default_provider_factory, resolve_provider};
pub use worker::{WorkerRuntime, load_libraries, start_worker_loop};
