//! application services that coordinate durable work for transport adapters.
//!
//! Each service is bound to the smallest store role it needs. HTTP handlers translate requests and
//! responses around these services; they do not reach through to the repository facade directly.

mod replica_registry;

pub use replica_registry::{
    DEFAULT_REPLICA_DELETE_SECONDS, DEFAULT_REPLICA_REAP_SECONDS, REPLICA_SAMPLE_RETENTION_SECONDS,
    REPLICA_STALE_SECONDS, ReplicaRegistry,
};

#[cfg(test)]
#[path = "replica_registry_tests.rs"]
mod replica_registry_tests;
