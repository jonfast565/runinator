#[allow(unused_imports)]
use super::*;

pub(crate) struct ValidationStore<S> {
    pub(super) inner: S,
    pub(super) alive: Arc<AtomicBool>,
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
