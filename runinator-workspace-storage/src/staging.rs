//! Single-spool immutable objects over an arbitrary base store.

use crate::{
    Error, Id, Result,
    cache::ByteCache,
    error::{corrupt, invalid},
    model::Kind,
    record,
    store::{Object, ObjectInfo, ReadStore, WriteStore},
};
use std::{
    collections::HashMap,
    fs::File,
    io::{Seek, Write},
    path::Path,
    sync::Mutex,
};

/// A readable overlay that can identify objects created by its current edit.

#[cfg(test)]
#[path = "staging_tests.rs"]
mod tests;

mod empty_store;
pub use empty_store::EmptyStore;

mod staged_store;
pub use staged_store::StagedStore;

mod staging;
pub use staging::Staging;

mod staged_object;
use staged_object::StagedObject;

mod state;
use state::State;
