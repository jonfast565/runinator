//! Broker-backed UI-event publication and optional embedded-engine latency signals.
//!
//! UI events are durable transport hints that every web-service replica consumes from the broker's
//! fan-out channel. The two [`EmbeddedEngineSignals`] notifications are deliberately different:
//! they exist only when a web service embeds an engine in the same process, and durable polling is
//! still the backstop when no such engine exists.

use std::sync::Arc;

use runinator_comm::{UiEvent, UiEventKind};
use runinator_models::runs::RunStatus;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{Broker, EventMessage};

/// A reusable, broker-backed publisher for best-effort UI events.
///
/// The publisher owns no web-service broadcast state and no engine-loop controls, so an embedded
/// web service and a standalone engine worker can share exactly the same event path.
#[derive(Clone)]
pub struct UiEventPublisher {
    broker: Arc<dyn Broker>,
}

impl UiEventPublisher {
    pub fn new(broker: Arc<dyn Broker>) -> Self {
        Self { broker }
    }

    /// Publish without delaying the caller's durable operation. A failed UI hint is logged, while
    /// the durable state change that caused it remains the source of truth for a later resync.
    pub fn emit(&self, event: UiEvent) {
        let broker = self.broker.clone();
        tokio::spawn(async move {
            if let Err(err) = broker.publish_event(EventMessage::new(event)).await {
                log::warn!("failed to publish UI event: {err}");
            }
        });
    }
}

/// Process-local signals shared only by the composition root that embeds an engine.
///
/// They reduce the time until durable work is polled after an HTTP write. Standalone engines receive
/// no handle and remain correct through their normal polling intervals.
#[derive(Clone, Default)]
pub struct EmbeddedEngineSignals {
    workflow_vm: Arc<Notify>,
    agent_directives: Arc<Notify>,
}

impl EmbeddedEngineSignals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Prompt the VM driver to check its durable continuation queue now.
    pub fn nudge_workflow_vm(&self) {
        self.workflow_vm.notify_one();
    }

    /// Prompt the agent-directive publisher to drain its durable outbox now.
    pub fn nudge_agent_directives(&self) {
        self.agent_directives.notify_one();
    }

    /// A signal the engine loop can await. Kept separate from the public nudge method so callers
    /// can only reduce latency, never take responsibility for consuming durable work.
    pub fn workflow_vm_notifier(&self) -> Arc<Notify> {
        self.workflow_vm.clone()
    }

    /// A signal the engine loop can await for its durable agent-directive outbox.
    pub fn agent_directives_notifier(&self) -> Arc<Notify> {
        self.agent_directives.clone()
    }
}

pub use runinator_comm::{UiEvent as AppEvent, UiEventKind as AppEventKind};

pub fn emit(events: &UiEventPublisher, event: AppEvent) {
    events.emit(event);
}

pub fn emit_workflow_run(events: &UiEventPublisher, run_id: Uuid, org_id: Option<Uuid>) {
    emit(
        events,
        AppEvent::new(org_id, AppEventKind::WorkflowRunChanged { run_id }),
    );
}

pub fn emit_pipeline_run(events: &UiEventPublisher, run_id: Uuid, org_id: Option<Uuid>) {
    emit(
        events,
        AppEvent::new(org_id, AppEventKind::PipelineRunChanged { run_id }),
    );
}

pub fn emit_task_run(
    events: &UiEventPublisher,
    run_id: Uuid,
    status: RunStatus,
    org_id: Option<Uuid>,
) {
    emit(
        events,
        AppEvent::new(
            org_id,
            AppEventKind::RunStatusChanged {
                run_id,
                terminal: is_terminal_run_status(status),
            },
        ),
    );
    // Tasks are a platform/ops surface, so their coarse list invalidation stays global.
    emit(events, AppEvent::global(AppEventKind::TasksChanged));
}

pub fn emit_workflows_changed(events: &UiEventPublisher, org_id: Option<Uuid>) {
    emit(events, AppEvent::new(org_id, UiEventKind::WorkflowsChanged));
}

pub fn is_terminal_run_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    #[tokio::test]
    async fn embedded_signals_retain_one_pending_nudge_per_loop() {
        let signals = EmbeddedEngineSignals::new();
        signals.nudge_workflow_vm();
        signals.nudge_agent_directives();

        tokio::time::timeout(
            Duration::from_secs(1),
            signals.workflow_vm_notifier().notified(),
        )
        .await
        .expect("workflow VM nudge should retain a permit");
        tokio::time::timeout(
            Duration::from_secs(1),
            signals.agent_directives_notifier().notified(),
        )
        .await
        .expect("agent directive nudge should retain a permit");
    }
}
