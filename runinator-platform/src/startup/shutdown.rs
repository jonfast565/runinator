#[allow(unused_imports)]
use super::*;

/// A process-wide, cloneable graceful-shutdown signal.
///
/// `install` listens once for Ctrl-C and, on Unix, SIGTERM. Existing services that accept an
/// `Arc<Notify>` can use [`Self::notifier`] during their incremental migration.
#[derive(Clone, Debug)]
pub struct Shutdown {
    pub(super) notify: Arc<Notify>,
    pub(super) cancelled: Arc<AtomicBool>,
}

impl Shutdown {
    pub fn install() -> Self {
        let shutdown = Self::new();
        let listener = shutdown.clone();
        tokio::spawn(async move {
            wait_for_signal().await;
            info!("shutdown signal received");
            listener.trigger();
        });
        shutdown
    }

    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn trigger(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub async fn cancelled(&self) {
        while !self.cancelled.load(Ordering::Acquire) {
            self.notify.notified().await;
        }
    }

    /// Whether graceful shutdown has been requested. Long-running optional UI helpers use this to
    /// restore their terminal state promptly after Ctrl-C/SIGTERM without becoming another owner
    /// of process shutdown.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn notifier(&self) -> Arc<Notify> {
        Arc::clone(&self.notify)
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}
