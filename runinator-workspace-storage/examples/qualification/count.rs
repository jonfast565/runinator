#[allow(unused_imports)]
use super::*;

pub(super) struct Count<S> {
    pub(super) inner: S,
    pub(super) bytes: AtomicU64,
}

impl<S: ReadStore> ReadStore for Count<S> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.inner.info(id)
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        let object = self.inner.get(id)?;
        self.bytes
            .fetch_add(object.bytes.len() as u64, Ordering::Relaxed);
        Ok(object)
    }
}
