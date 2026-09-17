//! Broker-backed UI-event publication and optional embedded-engine latency signals.
//!
//! UI events are durable transport hints that every web-service replica consumes from the broker's
//! fan-out channel. The two [`EmbeddedEngineSignals`] notifications are deliberately different:
//! they exist only when a web service embeds an engine in the same process, and durable polling is
//! still the backstop when no such engine exists.

use std::sync::Arc;

use runinator_comm::{UiEvent, UiEventKind};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{Broker, EventMessage};

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

pub fn emit_orchestration(events: &UiEventPublisher, orchestration_id: Uuid, org_id: Option<Uuid>) {
    emit(
        events,
        AppEvent::new(
            org_id,
            AppEventKind::OrchestrationChanged { orchestration_id },
        ),
    );
}

pub fn emit_adapter(events: &UiEventPublisher, adapter_id: Uuid, org_id: Option<Uuid>) {
    emit(
        events,
        AppEvent::new(org_id, AppEventKind::AdapterChanged { adapter_id }),
    );
}

pub fn emit_external_operation(
    events: &UiEventPublisher,
    operation_id: Uuid,
    orchestration_id: Uuid,
    org_id: Option<Uuid>,
) {
    emit(
        events,
        AppEvent::new(
            org_id,
            AppEventKind::ExternalOperationChanged {
                operation_id,
                orchestration_id,
            },
        ),
    );
}

pub fn emit_workflows_changed(events: &UiEventPublisher, org_id: Option<Uuid>) {
    emit(events, AppEvent::new(org_id, UiEventKind::WorkflowsChanged));
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

mod ui_event_publisher;
pub use ui_event_publisher::UiEventPublisher;

mod embedded_engine_signals;
pub use embedded_engine_signals::EmbeddedEngineSignals;
