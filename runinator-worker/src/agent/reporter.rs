//! publishes lifecycle state to the host observer and to anyone holding an [`crate::agent::AgentHandle`].
//!
//! one place decides what a transition looks like, so a headless log line and a gui status header
//! can never disagree about what the agent is doing.

use std::sync::Arc;

use tokio::sync::watch;
use tracing::info;

use crate::agent::observer::AgentObserver;
use crate::agent::status::{AgentConnection, AgentStatus};
use crate::events::WorkerEvent;

pub struct StatusReporter {
    observer: Arc<dyn AgentObserver>,
    state: watch::Sender<AgentStatus>,
}

impl StatusReporter {
    pub fn new(observer: Arc<dyn AgentObserver>, initial: AgentStatus) -> Self {
        Self {
            observer,
            state: watch::Sender::new(initial),
        }
    }

    /// emit a lifecycle line. also logged through tracing, which is the headless host's only view.
    pub fn log(&self, line: impl Into<String>) {
        let line = line.into();
        info!(target: "runinator_agent", "{line}");
        self.observer.on_log(&line);
    }

    pub fn set_connection(&self, connection: AgentConnection) {
        self.update(|status| status.connection = connection);
    }

    /// mutate the status and republish it. the observer is notified after the lock is released, so a
    /// host that reads the handle from its hook cannot deadlock against the publisher.
    pub fn update(&self, apply: impl FnOnce(&mut AgentStatus)) {
        self.state.send_modify(apply);
        let status = self.state.borrow().clone();
        self.observer.on_status(&status);
    }

    pub fn status(&self) -> AgentStatus {
        self.state.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<AgentStatus> {
        self.state.subscribe()
    }

    pub fn worker_event(&self, event: &WorkerEvent) {
        self.observer.on_worker_event(event);
    }
}
