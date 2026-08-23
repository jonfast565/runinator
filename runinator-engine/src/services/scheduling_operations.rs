//! application service for freeze windows and durable trigger backfills.

use std::sync::Arc;

use runinator_broker_core::{
    AppEvent, AppEventKind, EmbeddedEngineSignals, UiEventPublisher, emit, emit_workflow_run,
};
use runinator_models::{
    errors::SendableError,
    schedules::{BackfillRequest, BackfillResponse, FreezeWindow, NewFreezeWindow},
    web::TaskResponse,
    workflows::WorkflowDefinition,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, ScheduleStore},
};
use uuid::Uuid;

use crate::repository;

/// Coordinates schedule mutations with their UI invalidations and optional embedded-engine nudge.
#[derive(Clone)]
pub struct SchedulingOperations<T> {
    store: Arc<T>,
    events: UiEventPublisher,
    signals: Option<EmbeddedEngineSignals>,
}

impl<T> SchedulingOperations<T> {
    pub fn new(
        store: Arc<T>,
        events: UiEventPublisher,
        signals: Option<EmbeddedEngineSignals>,
    ) -> Self {
        Self {
            store,
            events,
            signals,
        }
    }

    fn schedules_changed(&self) {
        emit(
            &self.events,
            AppEvent::global(AppEventKind::SchedulesChanged),
        );
    }

    fn nudge_workflow_vm(&self) {
        if let Some(signals) = &self.signals {
            signals.nudge_workflow_vm();
        }
    }
}

impl<T: RuntimeStore + DefinitionStore + ScheduleStore> SchedulingOperations<T> {
    pub async fn list_freeze_windows(
        &self,
        org_id: Option<Uuid>,
        active: bool,
    ) -> Result<Vec<FreezeWindow>, SendableError> {
        if active {
            repository::fetch_active_freeze_windows(self.store.as_ref()).await
        } else {
            repository::fetch_freeze_windows(self.store.as_ref(), org_id).await
        }
    }

    pub async fn fetch_freeze_window(
        &self,
        window_id: Uuid,
    ) -> Result<Option<FreezeWindow>, SendableError> {
        repository::fetch_freeze_window(self.store.as_ref(), window_id).await
    }

    pub async fn workflow(
        &self,
        workflow_id: Uuid,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        repository::fetch_workflow(self.store.as_ref(), workflow_id).await
    }

    pub async fn create_freeze_window(
        &self,
        window: &NewFreezeWindow,
    ) -> Result<FreezeWindow, SendableError> {
        let saved = repository::create_freeze_window(self.store.as_ref(), window).await?;
        self.schedules_changed();
        Ok(saved)
    }

    pub async fn update_freeze_window(
        &self,
        window_id: Uuid,
        window: &NewFreezeWindow,
    ) -> Result<Option<FreezeWindow>, SendableError> {
        let saved =
            repository::update_freeze_window(self.store.as_ref(), window_id, window).await?;
        if saved.is_some() {
            self.schedules_changed();
        }
        Ok(saved)
    }

    pub async fn delete_freeze_window(
        &self,
        window_id: Uuid,
    ) -> Result<TaskResponse, SendableError> {
        let response = repository::delete_freeze_window(self.store.as_ref(), window_id).await?;
        self.schedules_changed();
        Ok(response)
    }

    pub fn validate_backfill(&self, request: &BackfillRequest) -> Result<(), SendableError> {
        repository::validate_backfill_request(request)
    }

    pub async fn backfill_workflow_trigger(
        &self,
        trigger_id: Uuid,
        request: &BackfillRequest,
    ) -> Result<BackfillResponse, SendableError> {
        let (response, runs) =
            repository::backfill_workflow_trigger(self.store.as_ref(), trigger_id, request).await?;
        for run in &runs {
            let org_id = repository::org_id_for_workflow_run(self.store.as_ref(), run.id).await;
            emit_workflow_run(&self.events, run.id, org_id);
        }
        if !runs.is_empty() {
            self.nudge_workflow_vm();
        }
        Ok(response)
    }
}
