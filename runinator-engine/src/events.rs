//! Engine-specific UI-event helpers.
//!
//! Broker publication itself lives in `runinator-broker-core::ui_events`, where the web service and
//! standalone engine share it. This module keeps only the engine's store-backed scope resolution.

use uuid::Uuid;

use runinator_broker_core::UiEventPublisher;
use runinator_store::RuntimeStore;

use crate::repository;

pub use runinator_broker_core::{
    AppEvent, AppEventKind, emit, emit_adapter, emit_external_operation, emit_orchestration,
    emit_pipeline_run, emit_workflow_run, emit_workflows_changed,
};

// Keep the threaded handle named EventSender so engine loop signatures remain descriptive without
// owning a second publisher abstraction.
pub type EventSender = UiEventPublisher;

pub async fn emit_workflow_run_resolved<T: RuntimeStore>(
    db: &T,
    events: &EventSender,
    run_id: Uuid,
) {
    let org_id = repository::org_id_for_workflow_run(db, run_id).await;
    emit_workflow_run(events, run_id, org_id);
}

pub async fn emit_pipeline_run_resolved<T: RuntimeStore>(
    db: &T,
    events: &EventSender,
    run_id: Uuid,
) {
    let org_id = repository::org_id_for_pipeline_run(db, run_id).await;
    emit_pipeline_run(events, run_id, org_id);
}

pub fn emit_workflow_run_activity(events: &EventSender, org_id: Option<Uuid>) {
    emit(
        events,
        AppEvent::new(org_id, AppEventKind::WorkflowRunActivity),
    );
}

pub fn emit_pipeline_run_activity(events: &EventSender, org_id: Option<Uuid>) {
    emit(
        events,
        AppEvent::new(org_id, AppEventKind::PipelineRunActivity),
    );
}
