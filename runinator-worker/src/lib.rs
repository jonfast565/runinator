//! the worker runtime: an action/control loop that resolves providers and executes task nodes,
//! publishing results back through the broker. exposed as a library so the standalone binary and an
//! embedded host (the desktop command center) can both drive the same loop.

pub mod agent;
pub mod broker;
pub mod config;
pub mod errors;
pub mod events;
pub mod executor;
pub mod idempotency;
mod lease;
pub mod metrics;
pub mod output_sink;
pub mod provider_repository;
pub mod secrets;
pub mod worker;

#[cfg(test)]
mod lib_tests;

pub use agent::{
    AgentConnection, AgentHandle, AgentMetrics, AgentObserver, AgentRuntime, AgentRuntimeConfig,
    AgentStatus, BrokerMode, BrokerSelection, CompletedAction, NoopObserver, derive_relay_url,
};
pub use broker::{BrokerConfig, build_broker};
pub use config::{Config, parse_config, parse_labels};
pub use events::{ActionOutcome, NoopEventSink, WorkerEvent, WorkerEventSink};
pub use provider_repository::{ProviderFactory, default_provider_factory, resolve_provider};
pub use worker::{WorkerRuntime, load_libraries, start_worker_loop};
