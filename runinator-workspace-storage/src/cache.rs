//! Byte-budgeted CLOCK caches. Arc ownership is the pin: a live returned Arc
//! prevents eviction; dropping it unpins without a manual counter to leak.
use crate::{
    Error, Id,
    error::{Result, corrupt},
    model::Kind,
    store::{Object, ObjectInfo, ReadStore},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Condvar, Mutex},
};

/// Independent metadata and chunk budgets prevent a sequential data scan from
/// evicting all namespace/index metadata.

/// Bounded read-through cache for backends whose info operation also requires an object fetch.
mod entry;
use entry::Entry;

mod state;
use state::State;

mod cache_stats;
pub use cache_stats::CacheStats;

mod byte_cache;
pub use byte_cache::ByteCache;

mod reservation;
use reservation::Reservation;

mod object_caches;
pub use object_caches::ObjectCaches;

mod cached_store;
pub use cached_store::CachedStore;

mod buffered_cache;
pub use buffered_cache::BufferedCache;

mod buffered_store;
pub use buffered_store::BufferedStore;
