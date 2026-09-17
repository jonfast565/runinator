#[allow(unused_imports)]
use super::*;

pub(crate) struct WorkspaceStorageContext<T> {
    pub(crate) store: Arc<T>,
    pub(crate) blobs: Arc<dyn runinator_blob_core::BlobStore>,
    pub(crate) limits: Arc<RwLock<WorkspaceLimits>>,
}

impl<T> Clone for WorkspaceStorageContext<T> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            blobs: self.blobs.clone(),
            limits: self.limits.clone(),
        }
    }
}
