//! Checkout-scoped object transport and disposable local cache.
use runinator_api::capabilities::WorkspaceObjectTransport;
use runinator_workspace::storage::{
    self, Id,
    store::{Object, ObjectInfo, ReadStore},
};
use std::sync::Arc;
use std::{fs, io::Write, time::Instant};

/// The verified local subset of the remote store, used for pack deduplication.

#[cfg(test)]
#[path = "workspace_objects_tests.rs"]
mod workspace_objects_tests;

fn io_error(error: impl std::error::Error + Send + Sync + 'static) -> storage::Error {
    storage::Error::Io(std::io::Error::other(error))
}

/// Apply the action deadline even when every object access hits local staging.
mod local_objects;
pub(super) use local_objects::LocalObjects;

mod worker_objects;
pub use worker_objects::WorkerObjects;

mod cached_objects;
pub(super) use cached_objects::CachedObjects;

mod checked_store;
pub(super) use checked_store::CheckedStore;
