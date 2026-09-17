#[allow(unused_imports)]
use super::*;

pub struct ReconnectBudget {
    /// `None` retries indefinitely, for a host whose orchestrator restarts it on exit.
    pub(super) max_attempts: Option<u32>,
    pub(super) attempts: AtomicU32,
    /// why the last attempt failed, kept so the terminal state can say what went wrong.
    pub(super) reason: Mutex<String>,
    pub(super) spent: watch::Sender<bool>,
}

impl ReconnectBudget {
    pub fn new(max_attempts: Option<u32>) -> Self {
        Self {
            max_attempts: max_attempts.filter(|max| *max > 0),
            attempts: AtomicU32::new(0),
            reason: Mutex::new(String::new()),
            spent: watch::Sender::new(false),
        }
    }

    pub fn max_attempts(&self) -> Option<u32> {
        self.max_attempts
    }

    pub fn attempts(&self) -> u32 {
        self.attempts.load(Ordering::SeqCst)
    }

    /// why the most recent attempt failed; empty before anything has been charged.
    pub fn reason(&self) -> String {
        self.reason
            .lock()
            .map(|reason| reason.clone())
            .unwrap_or_default()
    }

    pub fn is_spent(&self) -> bool {
        *self.spent.borrow()
    }

    /// charge one consecutive failure. saturating, so an agent left running against a dead endpoint
    /// long enough to overflow the counter still reports a sane attempt number.
    pub fn charge(&self, reason: impl Into<String>) -> Charge {
        let reason = reason.into();
        if let Ok(mut slot) = self.reason.lock() {
            *slot = reason;
        }
        let attempt = self
            .attempts
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                Some(current.saturating_add(1))
            })
            .unwrap_or(0)
            .saturating_add(1);
        let spent = self.max_attempts.is_some_and(|max| attempt >= max);
        if spent {
            // `send_replace`, not `send`: the budget is charged from the supervisor before anything
            // subscribes, and `send` both fails *and leaves the value untouched* when there is no
            // receiver — which would leave a spent budget reading as unspent.
            self.spent.send_replace(true);
        }
        Charge { attempt, spent }
    }

    /// clear the count after a connection that actually worked. never un-spends a spent budget:
    /// giving up is terminal, and a late transport reconnect must not resurrect a stopping agent.
    pub fn reset(&self) {
        if self.is_spent() {
            return;
        }
        self.attempts.store(0, Ordering::SeqCst);
    }

    /// resolve once the budget is spent, immediately if it already is. a watch rather than a
    /// `Notify` so a spend that lands between the check and the wait cannot be missed.
    pub async fn wait_spent(&self) {
        let mut spent = self.spent.subscribe();
        loop {
            if *spent.borrow_and_update() {
                return;
            }
            if spent.changed().await.is_err() {
                return;
            }
        }
    }
}
