#[allow(unused_imports)]
use super::*;

pub struct ReaderGuard<T: DurableWorkspaceStore> {
    pub(super) db: Arc<T>,
    pub(super) lease: WorkspaceReaderLease,
    pub(super) task: tokio::task::JoinHandle<()>,
    pub(super) runtime: tokio::runtime::Handle,
    pub(super) expires: Arc<std::sync::atomic::AtomicI64>,
}

impl<T: DurableWorkspaceStore> ReaderGuard<T> {
    pub async fn new(
        db: Arc<T>,
        workspace: uuid::Uuid,
        version: i64,
    ) -> Result<Self, SendableError> {
        let lease = db.pin_workspace_reader(workspace, version).await?;
        let expires = Arc::new(std::sync::atomic::AtomicI64::new(
            lease.expires_at.timestamp(),
        ));
        let renewing_db = db.clone();
        let renewing_lease = lease.clone();
        let renewing_expires = expires.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                match renewing_db
                    .renew_workspace_reader(renewing_lease.clone())
                    .await
                {
                    Ok(true) => {
                        renewing_expires.store(
                            (chrono::Utc::now() + chrono::Duration::minutes(4)).timestamp(),
                            std::sync::atomic::Ordering::Release,
                        );
                    }
                    _ => {
                        renewing_expires.store(0, std::sync::atomic::Ordering::Release);
                        return;
                    }
                }
            }
        });
        Ok(Self {
            db,
            lease,
            task,
            runtime: tokio::runtime::Handle::current(),
            expires,
        })
    }
    pub(super) fn alive(&self) -> bool {
        self.expires.load(std::sync::atomic::Ordering::Acquire) > chrono::Utc::now().timestamp()
    }
}

impl<T: DurableWorkspaceStore> Drop for ReaderGuard<T> {
    fn drop(&mut self) {
        self.task.abort();
        let db = self.db.clone();
        let id = self.lease.id;
        self.runtime.spawn(async move {
            let _ = db.release_workspace_reader(id).await;
        });
    }
}
