#[allow(unused_imports)]
use super::*;

#[derive(Clone, Default)]
pub struct WorkspacePhaseReporter {
    pub(super) events: std::sync::Arc<std::sync::Mutex<Vec<WorkspacePhaseEvent>>>,
}

impl WorkspacePhaseReporter {
    pub fn start(&self, phase: impl Into<String>) -> WorkspacePhaseTimer {
        WorkspacePhaseTimer {
            reporter: self.clone(),
            phase: phase.into(),
            started_at: chrono::Utc::now(),
            started: std::time::Instant::now(),
            recorded: false,
        }
    }

    pub fn drain(&self) -> Vec<WorkspacePhaseEvent> {
        self.events
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }

    pub(super) fn record(&self, event: WorkspacePhaseEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
}
