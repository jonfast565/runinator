#[allow(unused_imports)]
use super::*;

pub struct WorkspacePhaseTimer {
    pub(super) reporter: WorkspacePhaseReporter,
    pub(super) phase: String,
    pub(super) started_at: chrono::DateTime<chrono::Utc>,
    pub(super) started: std::time::Instant,
    pub(super) recorded: bool,
}

impl WorkspacePhaseTimer {
    pub fn succeeded(mut self, details: Value) {
        self.finish("succeeded", details);
    }

    pub(super) fn finish(&mut self, status: &str, details: Value) {
        self.recorded = true;
        self.reporter.record(WorkspacePhaseEvent {
            version: 1,
            phase: self.phase.clone(),
            status: status.into(),
            started_at: self.started_at,
            finished_at: chrono::Utc::now(),
            duration_ms: self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            details,
        });
    }
}

impl Drop for WorkspacePhaseTimer {
    fn drop(&mut self) {
        if self.recorded {
            return;
        }

        self.finish(
            "failed",
            runinator_models::json!({"message": "phase did not complete"}),
        );
    }
}
