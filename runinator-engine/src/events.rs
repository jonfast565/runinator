use std::sync::Arc;
use uuid::Uuid;

use runinator_broker::{Broker, EventMessage};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::runs::RunStatus;

use crate::repository;

// the UI event contract lives in runinator-comm so it can cross the broker fan-out events channel.
pub use runinator_comm::UiEvent as AppEvent;

/// publishes UI events onto the broker fan-out `events` channel. the web service's per-replica event
/// consumer re-broadcasts each event to that replica's WebSocket clients, so an out-of-process engine
/// can emit events and every ws replica's clients still see them.
#[derive(Clone)]
pub struct EnginePublisher {
    broker: Arc<dyn Broker>,
}

impl EnginePublisher {
    pub fn new(broker: Arc<dyn Broker>) -> Self {
        Self { broker }
    }
}

// keep the threaded handle named EventSender so the moved loop signatures are unchanged.
pub type EventSender = EnginePublisher;

pub fn emit(events: &EventSender, event: AppEvent) {
    // publish to the broker; the per-replica ws consumer re-broadcasts to every replica's clients.
    let broker = events.broker.clone();
    tokio::spawn(async move {
        if let Err(err) = broker.publish_event(EventMessage::new(event)).await {
            log::warn!("failed to publish UI event: {}", err);
        }
    });
}

pub fn emit_workflow_run(events: &EventSender, run_id: Uuid) {
    emit(events, AppEvent::WorkflowRunChanged { run_id });
}

pub fn emit_task_run(events: &EventSender, run_id: Uuid, status: RunStatus) {
    emit(
        events,
        AppEvent::RunStatusChanged {
            run_id,
            terminal: is_terminal_run_status(status),
        },
    );
    emit(events, AppEvent::TasksChanged);
}

pub fn is_terminal_run_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
    )
}

pub async fn emit_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    workflow_node_run_id: Uuid,
) {
    if let Ok(Some(node_run)) = repository::fetch_workflow_node_run(db, workflow_node_run_id).await
    {
        emit_workflow_run(events, node_run.workflow_run_id);
    }
}
