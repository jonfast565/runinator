//! Stop blocking validation after its owning request is dropped or expires.
use runinator_workspace::storage::{
    self, Id,
    store::{Object, ObjectInfo, ReadStore},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct ValidationGuard(Arc<AtomicBool>);

impl ValidationGuard {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }

    pub fn store<S>(&self, inner: S) -> ValidationStore<S> {
        ValidationStore {
            inner,
            alive: self.0.clone(),
        }
    }
}

impl Drop for ValidationGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(super) struct ValidationStore<S> {
    inner: S,
    alive: Arc<AtomicBool>,
}

impl<S: ReadStore> ReadStore for ValidationStore<S> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(storage::Error::Conflict);
        }
        self.inner.info(id)
    }

    fn get(&self, id: Id) -> storage::Result<Object> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(storage::Error::Conflict);
        }
        self.inner.get(id)
    }
}

#[cfg(test)]
#[path = "workspace_validation_tests.rs"]
mod tests;
