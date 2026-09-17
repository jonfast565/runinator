#[allow(unused_imports)]
use super::*;

/// live action counters folded from the worker event stream. `cpu_percent`/`mem_percent` are filled
/// by the telemetry sampler rather than by events.
#[derive(Debug, Clone, Default)]
pub struct AgentMetrics {
    pub in_flight: u32,
    pub succeeded: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub canceled: u64,
    pub skipped_duplicates: u64,
    pub last_completed: Option<CompletedAction>,
    pub cpu_percent: Option<f32>,
    pub mem_percent: Option<f32>,
}

impl AgentMetrics {
    /// fold one worker-loop event into the counters. saturating throughout: a counter that drifted
    /// (a finish with no matching start after a restart) must never panic the event sink.
    pub fn apply(&mut self, event: &WorkerEvent) {
        match event {
            WorkerEvent::EffectStarted { .. } => {
                self.in_flight = self.in_flight.saturating_add(1);
            }
            WorkerEvent::EffectSkippedDuplicate { .. } => {
                self.skipped_duplicates = self.skipped_duplicates.saturating_add(1);
            }
            WorkerEvent::EffectFinished {
                workflow_run_id,
                provider,
                function,
                effect_id,
                outcome,
                duration_ms,
                ..
            } => {
                self.in_flight = self.in_flight.saturating_sub(1);
                match outcome {
                    ActionOutcome::Succeeded => self.succeeded = self.succeeded.saturating_add(1),
                    ActionOutcome::Failed => self.failed = self.failed.saturating_add(1),
                    ActionOutcome::TimedOut => self.timed_out = self.timed_out.saturating_add(1),
                    ActionOutcome::Canceled => self.canceled = self.canceled.saturating_add(1),
                }
                self.last_completed = Some(CompletedAction {
                    summary: format!(
                        "{provider}.{function} (effect {}, run {})",
                        short_id(effect_id),
                        short_id(workflow_run_id)
                    ),
                    outcome: *outcome,
                    duration_ms: *duration_ms,
                });
            }
            WorkerEvent::EffectOutputChunk { .. } | WorkerEvent::ControlReceived { .. } => {}
        }
    }
}
