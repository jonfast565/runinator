use runinator_database::interfaces::DatabaseImpl;
use runinator_models::runs::RunStatus;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::repository;

#[derive(Clone, Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    RunStatusChanged { run_id: i64, terminal: bool },
    RunChunkAdded { run_id: i64 },
    WorkflowsChanged,
    WorkflowRunChanged { run_id: i64 },
    WorkflowRunActivity,
    TasksChanged,
    ArtifactCreated { artifact_id: i64, run_id: i64 },
    NotificationCreated { notification_id: i64 },
    NotificationsChanged,
}

pub type EventSender = broadcast::Sender<AppEvent>;

pub(crate) fn emit(events: &EventSender, event: AppEvent) {
    let _ = events.send(event);
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
