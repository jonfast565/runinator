//! application service for freeze windows and durable trigger backfills.

use std::sync::Arc;

use runinator_broker_core::{
    AppEvent, AppEventKind, EmbeddedEngineSignals, UiEventPublisher, emit, emit_workflow_run,
    emit_workflows_changed,
};
use runinator_models::{
    auth::User,
    errors::SendableError,
    pipelines::{Pipeline, PipelineTrigger},
    rbac::RoleAssignment,
    schedules::{
        BackfillRequest, BackfillResponse, CalendarSubscription, FreezeWindow,
        NewCalendarSubscriptionRecord, NewFreezeWindow, TriggerFiringBatch,
    },
    web::TaskResponse,
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowTrigger},
};
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, DefinitionStore, RbacStore, ScheduleStore},
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
    pub async fn workflows(&self) -> Result<Vec<WorkflowDefinition>, SendableError> {
        repository::fetch_calendar_workflows(self.store.as_ref()).await
    }

    pub async fn pipelines(&self) -> Result<Vec<Pipeline>, SendableError> {
        repository::fetch_calendar_pipelines(self.store.as_ref()).await
    }

    pub async fn list_pipeline_triggers(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<PipelineTrigger>, SendableError> {
        repository::fetch_calendar_pipeline_triggers(self.store.as_ref(), pipeline_id).await
    }

    pub async fn create_calendar_subscription(
        &self,
        record: &NewCalendarSubscriptionRecord,
    ) -> Result<CalendarSubscription, SendableError> {
        repository::create_calendar_subscription(self.store.as_ref(), record).await
    }

    pub async fn fetch_calendar_subscription_by_hash(
        &self,
        token_hash: String,
    ) -> Result<Option<CalendarSubscription>, SendableError> {
        repository::fetch_calendar_subscription_by_hash(self.store.as_ref(), token_hash).await
    }

    pub async fn delete_calendar_subscription(
        &self,
        subscription_id: Uuid,
        principal_id: Uuid,
    ) -> Result<bool, SendableError> {
        repository::delete_calendar_subscription(self.store.as_ref(), subscription_id, principal_id)
            .await
    }

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

    pub async fn list_workflow_triggers(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        repository::fetch_workflow_triggers(self.store.as_ref(), workflow_id).await
    }

    pub async fn fetch_workflow_trigger(
        &self,
        trigger_id: Uuid,
    ) -> Result<Option<WorkflowTrigger>, SendableError> {
        repository::fetch_workflow_trigger(self.store.as_ref(), trigger_id).await
    }

    pub async fn due_workflow_triggers(&self) -> Result<Vec<WorkflowTrigger>, SendableError> {
        repository::fetch_due_workflow_triggers(self.store.as_ref()).await
    }

    pub async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: String,
        limit: i64,
    ) -> Result<TriggerFiringBatch<WorkflowRun>, SendableError> {
        repository::claim_due_workflow_trigger_firings(self.store.as_ref(), scheduler_id, limit)
            .await
    }

    pub async fn save_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
        fallback_org_id: Option<Uuid>,
    ) -> Result<WorkflowTrigger, SendableError> {
        let saved = repository::upsert_workflow_trigger(self.store.as_ref(), trigger).await?;
        let org_id = match self.workflow(saved.workflow_id).await {
            Ok(Some(workflow)) => workflow.org_id.or(fallback_org_id),
            _ => fallback_org_id,
        };
        emit_workflows_changed(&self.events, org_id);
        Ok(saved)
    }

    pub async fn delete_workflow_trigger(
        &self,
        trigger_id: Uuid,
        fallback_org_id: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let org_id = match self.fetch_workflow_trigger(trigger_id).await {
            Ok(Some(trigger)) => match self.workflow(trigger.workflow_id).await {
                Ok(Some(workflow)) => workflow.org_id.or(fallback_org_id),
                _ => fallback_org_id,
            },
            _ => fallback_org_id,
        };
        let response = repository::delete_workflow_trigger(self.store.as_ref(), trigger_id).await?;
        emit_workflows_changed(&self.events, org_id);
        Ok(response)
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

impl<T: AuthStore + RbacStore> SchedulingOperations<T> {
    pub async fn calendar_user(&self, user_id: Uuid) -> Result<Option<User>, SendableError> {
        repository::fetch_calendar_user(self.store.as_ref(), user_id).await
    }

    pub async fn calendar_role_assignments(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<RoleAssignment>, SendableError> {
        repository::fetch_calendar_role_assignments(self.store.as_ref(), user_id).await
    }
}
