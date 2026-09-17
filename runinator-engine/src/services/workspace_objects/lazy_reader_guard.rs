#[allow(unused_imports)]
use super::*;

pub struct LazyReaderGuard<T: DurableWorkspaceStore> {
    pub(super) db: Arc<T>,
    pub(super) workspace: uuid::Uuid,
    pub(super) version: i64,
    pub(super) guard: std::sync::Mutex<Option<ReaderGuard<T>>>,
}

impl<T: DurableWorkspaceStore> LazyReaderGuard<T> {
    pub fn new(db: Arc<T>, workspace: uuid::Uuid, version: i64) -> Self {
        Self {
            db,
            workspace,
            version,
            guard: std::sync::Mutex::new(None),
        }
    }

    pub(super) fn ensure(&self, runtime: &tokio::runtime::Handle) -> storage::Result<()> {
        let mut guard = self.guard.lock().map_err(|_| storage::Error::Poisoned)?;
        if guard.as_ref().is_some_and(ReaderGuard::alive) {
            return Ok(());
        }
        *guard = Some(
            runtime
                .block_on(ReaderGuard::new(
                    self.db.clone(),
                    self.workspace,
                    self.version,
                ))
                .map_err(storage_error)?,
        );
        Ok(())
    }

    pub(super) fn alive(&self) -> bool {
        self.guard
            .lock()
            .is_ok_and(|guard| guard.as_ref().is_some_and(ReaderGuard::alive))
    }
}
