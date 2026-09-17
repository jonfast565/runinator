#[allow(unused_imports)]
use super::*;

pub(crate) struct CheckedStore<S> {
    pub inner: S,
    pub deadline: std::sync::Arc<WorkerObjects>,
}

impl<S: ReadStore> ReadStore for CheckedStore<S> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.deadline.remaining()?;
        self.inner.info(id)
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        self.deadline.remaining()?;
        self.inner.get(id)
    }
    fn contains(&self, id: Id) -> storage::Result<bool> {
        self.deadline.remaining()?;
        self.inner.contains(id)
    }
}

impl<S: storage::store::WriteStore> storage::store::WriteStore for CheckedStore<S> {
    fn put(&self, kind: storage::model::Kind, bytes: &[u8]) -> storage::Result<Id> {
        self.deadline.remaining()?;
        self.inner.put(kind, bytes)
    }
}

impl<S: storage::staging::StagedStore> storage::staging::StagedStore for CheckedStore<S> {
    fn is_staged(&self, id: Id) -> storage::Result<bool> {
        self.deadline.remaining()?;
        self.inner.is_staged(id)
    }
}
