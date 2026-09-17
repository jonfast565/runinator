#[allow(unused_imports)]
use super::*;

pub(super) struct GuardedStore<S> {
    pub(super) inner: S,
    pub(super) alive: Arc<AtomicBool>,
}

impl<S: runinator_workspace::storage::store::ReadStore>
    runinator_workspace::storage::store::ReadStore for GuardedStore<S>
{
    fn get(
        &self,
        id: runinator_workspace::storage::Id,
    ) -> runinator_workspace::storage::Result<runinator_workspace::storage::store::Object> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(runinator_workspace::storage::Error::Conflict);
        }
        self.inner.get(id)
    }
    fn info(
        &self,
        id: runinator_workspace::storage::Id,
    ) -> runinator_workspace::storage::Result<runinator_workspace::storage::store::ObjectInfo> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(runinator_workspace::storage::Error::Conflict);
        }
        self.inner.info(id)
    }
}
