#[allow(unused_imports)]
use super::*;

pub(crate) struct WorkspaceTransferProgress {
    pub(crate) alive: Arc<AtomicBool>,
    pub(crate) bytes: Arc<AtomicU64>,
    pub(super) task: tokio::task::JoinHandle<()>,
}

impl Drop for WorkspaceTransferProgress {
    fn drop(&mut self) {
        self.task.abort();
        self.alive.store(false, Ordering::Release);
    }
}

impl WorkspaceTransferProgress {
    pub(super) fn start<T: DurableWorkspaceStore>(store: Arc<T>, job: WorkspaceTransfer) -> Self {
        let alive = Arc::new(AtomicBool::new(true));
        let bytes = Arc::new(AtomicU64::new(0));
        let valid = alive.clone();
        let count = bytes.clone();
        let task = tokio::spawn(async move {
            loop {
                if !matches!(
                    store
                        .progress_workspace_transfer(job.clone(), count.load(Ordering::Acquire))
                        .await,
                    Ok(true)
                ) {
                    valid.store(false, Ordering::Release);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
        Self { alive, bytes, task }
    }

    #[cfg(test)]
    pub(crate) fn testing() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(true)),
            bytes: Arc::new(AtomicU64::new(0)),
            task: tokio::spawn(std::future::pending()),
        }
    }
}
