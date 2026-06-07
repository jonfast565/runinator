use std::sync::Arc;

use runinator_broker::{Broker, EventMessage};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::runs::RunStatus;
use tokio::sync::broadcast;

use crate::repository;

// the UI event contract lives in runinator-comm so it can cross the broker fan-out events channel.
pub use runinator_comm::UiEvent as AppEvent;

/// fan-out bus for UI events. emitting publishes to the broker `events` channel; the per-replica
/// event consumer ([`crate::background::run_event_consumer`]) is the sole writer to the local
/// broadcast that feeds this replica's WebSocket clients. this keeps every ws replica's clients in
/// sync regardless of which replica did the work.
#[derive(Clone)]
pub struct EventBus {
    local: broadcast::Sender<AppEvent>,
    broker: Arc<dyn Broker>,
}

impl EventBus {
    pub fn new(local: broadcast::Sender<AppEvent>, broker: Arc<dyn Broker>) -> Self {
        Self { local, broker }
    }

    /// subscribe a WebSocket client to this replica's locally-broadcast events.
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.local.subscribe()
    }
}

// the threaded handle stays named EventSender so handler/loop signatures are unchanged.
pub type EventSender = EventBus;

pub(crate) fn emit(events: &EventSender, event: AppEvent) {
    // publish to the broker; the per-replica consumer re-broadcasts to every replica's clients.
    let broker = events.broker.clone();
    tokio::spawn(async move {
        if let Err(err) = broker.publish_event(EventMessage::new(event)).await {
            log::warn!("failed to publish UI event: {}", err);
        }
    });
}

pub(crate) fn emit_workflow_run(events: &EventSender, run_id: i64) {
    emit(events, AppEvent::WorkflowRunChanged { run_id });
}

pub(crate) fn emit_task_run(events: &EventSender, run_id: i64, status: RunStatus) {
    emit(
        events,
        AppEvent::RunStatusChanged {
            run_id,
            terminal: is_terminal_run_status(status),
        },
    );
    emit(events, AppEvent::TasksChanged);
}

pub(crate) fn is_terminal_run_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
    )
}

pub(crate) async fn emit_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    workflow_node_run_id: i64,
) {
    if let Ok(Some(node_run)) = repository::fetch_workflow_node_run(db, workflow_node_run_id).await
    {
        emit_workflow_run(events, node_run.workflow_run_id);
    }
}
