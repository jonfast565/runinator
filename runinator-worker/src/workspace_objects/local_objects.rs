#[allow(unused_imports)]
use super::*;

pub(crate) struct LocalObjects {
    pub(super) store: runinator_workspace::native::PackedStore,
    pub(super) _scratch: tempfile::TempDir,
}

impl LocalObjects {
    pub(crate) fn new(
        store: runinator_workspace::native::PackedStore,
        scratch: tempfile::TempDir,
    ) -> Self {
        Self {
            store,
            _scratch: scratch,
        }
    }
}

impl ReadStore for LocalObjects {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.store.info(id)
    }

    fn get(&self, id: Id) -> storage::Result<Object> {
        self.store.get(id)
    }
}
