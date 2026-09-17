//! Stop blocking validation after its owning request is dropped or expires.
use runinator_workspace::storage::{
    self, Id,
    store::{Object, ObjectInfo, ReadStore},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
#[path = "workspace_validation_tests.rs"]
mod tests;

mod validation_guard;
pub(super) use validation_guard::ValidationGuard;

mod validation_store;
pub(super) use validation_store::ValidationStore;
