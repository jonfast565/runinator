use std::sync::Arc;
use uuid::Uuid;

use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_engine::EnginePublisher;
use runinator_models::runs::RunStatus;
use tokio::sync::broadcast;

// the UI event contract lives in runinator-comm so it can cross the broker fan-out events channel.
pub use runinator_comm::{UiEvent as AppEvent, UiEventKind as AppEventKind};

/// fan-out bus for UI events. it keeps the local broadcast that feeds this replica's WebSocket
/// clients (via [`EventBus::subscribe`], written solely by
/// [`crate::event_consumer::run_event_consumer`]) and delegates every emit to the shared
/// [`EnginePublisher`], so ws handlers and the background engine publish onto the broker `events`
/// channel through one code path. this keeps every ws replica's clients in sync regardless of which
/// replica (or a standalone background worker) did the work.
#[derive(Clone)]
pub struct EventBus {
    local: broadcast::Sender<AppEvent>,
    publisher: EnginePublisher,
}

impl EventBus {
    pub fn new(local: broadcast::Sender<AppEvent>, broker: Arc<dyn Broker>) -> Self {
        Self::from_publisher(local, EnginePublisher::new(broker))
    }

    /// share an existing [`EnginePublisher`] so HTTP create handlers can nudge the in-process wake
    /// publisher owned by the same handle.
    pub fn from_publisher(local: broadcast::Sender<AppEvent>, publisher: EnginePublisher) -> Self {
        Self { local, publisher }
    }

    /// subscribe a WebSocket client to this replica's locally-broadcast events.
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.local.subscribe()
    }
}

// the threaded handle stays named EventSender so handler signatures are unchanged.
pub type EventSender = EventBus;

pub(crate) fn emit(events: &EventSender, event: AppEvent) {
    runinator_engine::events::emit(&events.publisher, event);
}

pub(crate) fn emit_workflow_run(events: &EventSender, run_id: Uuid, org_id: Option<Uuid>) {
    runinator_engine::events::emit_workflow_run(&events.publisher, run_id, org_id);
}

pub(crate) fn emit_pipeline_run(events: &EventSender, run_id: Uuid, org_id: Option<Uuid>) {
    runinator_engine::events::emit_pipeline_run(&events.publisher, run_id, org_id);
}

pub(crate) fn nudge_wake_publisher(events: &EventSender) {
    events.publisher.nudge_wake_publisher();
}

pub(crate) fn emit_task_run(
    events: &EventSender,
    run_id: Uuid,
    status: RunStatus,
    org_id: Option<Uuid>,
) {
    runinator_engine::events::emit_task_run(&events.publisher, run_id, status, org_id);
}

pub(crate) async fn emit_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    workflow_node_run_id: Uuid,
) {
    runinator_engine::events::emit_workflow_node_run(db, &events.publisher, workflow_node_run_id)
        .await;
}

pub(crate) fn emit_workflows_changed(events: &EventSender, org_id: Option<Uuid>) {
    runinator_engine::events::emit_workflows_changed(&events.publisher, org_id);
}
