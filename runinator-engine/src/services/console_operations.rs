//! application service for REXRAP console sessions and cells.

use std::sync::Arc;

use runinator_broker_core::{Broker, EmbeddedEngineSignals, UiEventPublisher, emit_workflow_run};
use runinator_models::{
    console::{ConsoleCell, ConsoleSession, ConsoleSessionDetail, NewConsoleCell},
    errors::SendableError,
    web::TaskResponse,
};
use runinator_store::{
    RuntimeStore,
    roles::{
        ConsoleStore, DefinitionStore, ExecutionProfileStore, FunctionStore, NotificationStore,
        ScheduleStore, WorkflowVmStore,
    },
};
use uuid::Uuid;

use crate::repository;

/// Coordinates console persistence with the workflow run that effectful cells create.
#[derive(Clone)]
pub struct ConsoleOperations<T> {
    store: Arc<T>,
    broker: Arc<dyn Broker>,
    events: UiEventPublisher,
    signals: Option<EmbeddedEngineSignals>,
}

impl<T> ConsoleOperations<T> {
    pub fn new(
        store: Arc<T>,
        broker: Arc<dyn Broker>,
        events: UiEventPublisher,
        signals: Option<EmbeddedEngineSignals>,
    ) -> Self {
        Self {
            store,
            broker,
            events,
            signals,
        }
    }

    async fn run_changed(&self, run_id: Uuid)
    where
        T: RuntimeStore,
    {
        let org_id = repository::org_id_for_workflow_run(self.store.as_ref(), run_id).await;
        emit_workflow_run(&self.events, run_id, org_id);
        if let Some(signals) = &self.signals {
            signals.nudge_workflow_vm();
        }
    }
}

impl<
    T: ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
> ConsoleOperations<T>
{
    pub async fn list_sessions(&self) -> Result<Vec<ConsoleSession>, SendableError> {
        repository::console::fetch_sessions(self.store.as_ref()).await
    }

    pub async fn create_session(
        &self,
        org_id: Option<Uuid>,
        name: &str,
        created_by: Option<Uuid>,
    ) -> Result<ConsoleSession, SendableError> {
        repository::console::create_session(self.store.as_ref(), org_id, name, created_by).await
    }

    pub async fn session_detail(
        &self,
        session_id: Uuid,
    ) -> Result<Option<ConsoleSessionDetail>, SendableError> {
        repository::console::fetch_session_detail(self.store.as_ref(), session_id).await
    }

    pub async fn rename_session(
        &self,
        session_id: Uuid,
        name: &str,
    ) -> Result<bool, SendableError> {
        repository::console::rename_session(self.store.as_ref(), session_id, name).await
    }

    pub async fn delete_session(&self, session_id: Uuid) -> Result<bool, SendableError> {
        repository::console::delete_session(self.store.as_ref(), session_id).await
    }

    pub async fn clear_session(&self, session_id: Uuid) -> Result<bool, SendableError> {
        repository::console::clear_session(self.store.as_ref(), session_id).await
    }

    pub async fn upsert_cell(
        &self,
        session_id: Uuid,
        cell_id: Option<Uuid>,
        cell: &NewConsoleCell,
    ) -> Result<ConsoleCell, SendableError> {
        repository::console::upsert_cell(self.store.as_ref(), session_id, cell_id, cell).await
    }

    pub async fn fetch_cell(&self, cell_id: Uuid) -> Result<Option<ConsoleCell>, SendableError> {
        repository::console::fetch_cell(self.store.as_ref(), cell_id).await
    }

    pub async fn delete_cell(&self, cell_id: Uuid) -> Result<bool, SendableError> {
        repository::console::delete_cell(self.store.as_ref(), cell_id).await
    }

    pub async fn run_cell(
        &self,
        cell_id: Uuid,
    ) -> Result<repository::console::CellOutcome, SendableError>
    where
        T: ExecutionProfileStore,
    {
        let providers =
            repository::fetch_catalog_items(self.store.as_ref(), Some("provider_metadata".into()))
                .await
                .and_then(|items| Ok(repository::provider_metadata_from_items(items)?))?;
        let functions = repository::functions::fetch_catalog(self.store.as_ref())
            .await
            .unwrap_or_default();
        let outcome =
            repository::console::run_cell(self.store.as_ref(), cell_id, providers, functions)
                .await?;
        if let Some(run) = &outcome.run {
            self.run_changed(run.id).await;
        }
        Ok(outcome)
    }

    pub async fn cancel_cell_run(&self, run_id: Uuid) -> Result<TaskResponse, SendableError> {
        let response =
            repository::cancel_workflow_run(self.store.as_ref(), self.broker.as_ref(), run_id)
                .await?;
        self.run_changed(run_id).await;
        Ok(response)
    }

    pub async fn settle_cell_for_run(
        &self,
        run_id: Uuid,
    ) -> Result<Option<ConsoleCell>, SendableError> {
        repository::console::settle_cell_for_run(self.store.as_ref(), run_id).await
    }
}
