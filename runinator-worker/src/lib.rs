//! the worker runtime: an action/control loop that resolves providers and executes task nodes,
//! publishing results back through the broker. exposed as a library so the standalone binary and an
//! embedded host (the desktop command center) can both drive the same loop.

pub mod broker;
pub mod config;
pub mod errors;
pub mod executor;
pub mod metrics;
pub mod output_sink;
pub mod provider_repository;
pub mod secrets;
pub mod worker;

#[cfg(test)]
mod tests;

pub use broker::build_broker;
pub use config::{Config, parse_config};
pub use provider_repository::{ProviderFactory, default_provider_factory, resolve_provider};
pub use worker::{WorkerRuntime, load_libraries, start_worker_loop};
