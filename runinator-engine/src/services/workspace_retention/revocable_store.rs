#[allow(unused_imports)]
use super::*;

pub(super) struct RevocableStore<S> {
    pub(super) inner: S,
    pub(super) valid: Arc<AtomicBool>,
}

impl<S: ReadStore> ReadStore for RevocableStore<S> {
    fn get_many(&self, ids: &[Id]) -> storage::Result<Vec<Object>> {
        if !self.valid.load(Ordering::Acquire) {
            return Err(storage::Error::Conflict);
        }
        self.inner.get_many(ids)
    }

    fn get(&self, id: Id) -> storage::Result<Object> {
        if !self.valid.load(Ordering::Acquire) {
            return Err(storage::Error::Conflict);
        }
        self.inner.get(id)
    }

    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        if !self.valid.load(Ordering::Acquire) {
            return Err(storage::Error::Conflict);
        }
        self.inner.info(id)
    }
}
