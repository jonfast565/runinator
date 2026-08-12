//! the agent's stop signal.
//!
//! a bare `Notify` is not enough on its own: `notify_waiters` only wakes waiters that already exist,
//! so a host that starts the agent and immediately stops it would leave the lifecycle task running
//! with nobody to tell. the latch makes the signal sticky, and the `Notify` keeps the wake
//! immediate — the worker loop takes the `Notify` directly, since that is what it already expects.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct Shutdown {
    notify: Arc<Notify>,
    stopping: Arc<AtomicBool>,
}

impl Shutdown {
    pub fn new() -> Self {
        Self::default()
    }

    /// latch the signal and wake every current waiter.
    pub fn trigger(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// the raw handle the worker loop waits on.
    pub fn notify(&self) -> Arc<Notify> {
        Arc::clone(&self.notify)
    }

    /// wait out `delay`, returning `true` if shutdown fired first (or had already fired), so a
    /// backoff never delays an intentional stop.
    pub async fn sleep_or_stop(&self, delay: Duration) -> bool {
        if self.is_stopping() {
            return true;
        }
        tokio::select! {
            _ = self.notify.notified() => true,
            _ = tokio::time::sleep(delay) => self.is_stopping(),
        }
    }
}

#[cfg(test)]
#[path = "shutdown_tests.rs"]
mod tests;
