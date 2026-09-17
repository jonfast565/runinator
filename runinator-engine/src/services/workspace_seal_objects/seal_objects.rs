#[allow(unused_imports)]
use super::*;

pub(crate) struct SealObjects<T: DurableWorkspaceStore>(
    pub(super) storage::cache::BufferedStore<PackObjects<T>>,
);

impl<T: DurableWorkspaceStore> SealObjects<T> {
    pub fn new(source: SharedObjects<T>) -> Self {
        Self(storage::cache::BufferedStore::new(
            PackObjects {
                source,
                packs: Mutex::new(VecDeque::new()),
                records: storage::cache::ByteCache::new(64 * 1024 * 1024),
                #[cfg(test)]
                database_reads: std::sync::atomic::AtomicU64::new(0),
                #[cfg(test)]
                blob_reads: std::sync::atomic::AtomicU64::new(0),
            },
            48 * 1024 * 1024,
        ))
    }
    #[cfg(test)]
    pub fn io_counts(&self) -> (u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.0.inner.database_reads.load(Ordering::Relaxed),
            self.0.inner.blob_reads.load(Ordering::Relaxed),
        )
    }
}

impl<T: DurableWorkspaceStore> ReadStore for SealObjects<T> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.0.info(id)
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        self.0.get(id)
    }
}
