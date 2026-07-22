use std::sync::Arc;
use uuid::Uuid;

use runinator_broker::{Broker, EventMessage};
use runinator_comm::UiEventKind;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::runs::RunStatus;
use tokio::sync::Notify;

use crate::repository;

// the UI event contract lives in runinator-comm so it can cross the broker fan-out events channel.
pub use runinator_comm::{UiEvent as AppEvent, UiEventKind as AppEventKind};

/// publishes UI events onto the broker fan-out `events` channel. the web service's per-replica event
/// consumer re-broadcasts each event to that replica's WebSocket clients, so an out-of-process engine
/// can emit events and every ws replica's clients still see them.
///
/// also owns the in-process wake and action-dispatch nudges: HTTP create handlers and engine loops
/// share one [`EnginePublisher`] so newly enqueued ready nodes / outbox rows can wake those publishers
/// without waiting for their poll intervals. when the engine runs in another process the remote
/// publishers still poll as a durable backstop.
#[derive(Clone)]
pub struct EnginePublisher {
    broker: Arc<dyn Broker>,
    wake_nudge: Arc<Notify>,
    action_nudge: Arc<Notify>,
}

impl EnginePublisher {
    pub fn new(broker: Arc<dyn Broker>) -> Self {
        Self {
            broker,
            wake_nudge: Arc::new(Notify::new()),
            action_nudge: Arc::new(Notify::new()),
        }
    }

    /// handle shared with [`crate::loops::run_wake_publisher`] so create/drive paths can interrupt
    /// the poll sleep.
    pub(crate) fn wake_nudge(&self) -> Arc<Notify> {
        self.wake_nudge.clone()
    }

    /// handle shared with [`crate::loops::run_action_dispatch_publisher`].
    pub(crate) fn action_nudge(&self) -> Arc<Notify> {
        self.action_nudge.clone()
    }

    /// wake the in-process wake publisher so newly enqueued ready nodes are announced promptly.
    pub fn nudge_wake_publisher(&self) {
        // notify_one stores a permit when nobody is waiting, so a nudge during publish is not lost.
        self.wake_nudge.notify_one();
    }

    /// wake the in-process action-dispatch publisher so outbox rows reach workers promptly.
    pub fn nudge_action_dispatch_publisher(&self) {
        self.action_nudge.notify_one();
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

pub fn emit_workflow_run(events: &EventSender, run_id: Uuid, org_id: Option<Uuid>) {
    emit(
        events,
        AppEvent::new(org_id, AppEventKind::WorkflowRunChanged { run_id }),
    );
}

pub fn emit_pipeline_run(events: &EventSender, run_id: Uuid, org_id: Option<Uuid>) {
    emit(
        events,
        AppEvent::new(org_id, AppEventKind::PipelineRunChanged { run_id }),
    );
}

pub fn emit_task_run(events: &EventSender, run_id: Uuid, status: RunStatus, org_id: Option<Uuid>) {
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
    // tasks list is a platform/ops surface — keep global.
    emit(events, AppEvent::global(AppEventKind::TasksChanged));
}

pub fn is_terminal_run_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
    )
}

pub async fn emit_workflow_run_resolved<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    run_id: Uuid,
) {
    let org_id = repository::org_id_for_workflow_run(db, run_id).await;
    emit_workflow_run(events, run_id, org_id);
}

pub async fn emit_pipeline_run_resolved<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    run_id: Uuid,
) {
    let org_id = repository::org_id_for_pipeline_run(db, run_id).await;
    emit_pipeline_run(events, run_id, org_id);
}

pub async fn emit_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    workflow_node_run_id: Uuid,
) {
    if let Ok(Some(node_run)) = repository::fetch_workflow_node_run(db, workflow_node_run_id).await
    {
        emit_workflow_run_resolved(db, events, node_run.workflow_run_id).await;
    }
}

/// emit a coarse workflows-changed tip scoped to `org_id` (active/resource org). unscoped when None.
pub fn emit_workflows_changed(events: &EventSender, org_id: Option<Uuid>) {
    emit(events, AppEvent::new(org_id, UiEventKind::WorkflowsChanged));
}

pub fn emit_workflow_run_activity(events: &EventSender, org_id: Option<Uuid>) {
    emit(events, AppEvent::new(org_id, UiEventKind::WorkflowRunActivity));
}

pub fn emit_pipeline_run_activity(events: &EventSender, org_id: Option<Uuid>) {
    emit(events, AppEvent::new(org_id, UiEventKind::PipelineRunActivity));
}
