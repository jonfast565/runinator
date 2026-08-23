//! application service for continuation-scoped workflow debugger commands.

use std::sync::Arc;

use runinator_broker_core::{EmbeddedEngineSignals, UiEventPublisher, emit_workflow_run};
use runinator_comm::DebugVerb;
use runinator_models::{errors::SendableError, web::TaskResponse};
use runinator_store::{RuntimeStore, roles::WorkflowVmStore};
use uuid::Uuid;

use crate::repository;

/// Applies durable debug commands and publishes the corresponding run invalidation.
#[derive(Clone)]
pub struct DebugOperations<T> {
    store: Arc<T>,
    events: UiEventPublisher,
    signals: Option<EmbeddedEngineSignals>,
}

impl<T> DebugOperations<T> {
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

    async fn changed(&self, workflow_run_id: Uuid)
    where
        T: RuntimeStore,
    {
        let org_id =
            repository::org_id_for_workflow_run(self.store.as_ref(), workflow_run_id).await;
        emit_workflow_run(&self.events, workflow_run_id, org_id);
        if let Some(signals) = &self.signals {
            signals.nudge_workflow_vm();
        }
    }
}

impl<T: RuntimeStore + WorkflowVmStore> DebugOperations<T> {
    pub async fn command(
        &self,
        workflow_run_id: Uuid,
        verb: DebugVerb,
    ) -> Result<TaskResponse, SendableError> {
        let response =
            repository::apply_debug_command(self.store.as_ref(), workflow_run_id, verb).await?;
        self.changed(workflow_run_id).await;
        Ok(response)
    }

    pub async fn step(
        &self,
        workflow_run_id: Uuid,
        cursor: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let response =
            repository::step_debug_cursor(self.store.as_ref(), workflow_run_id, cursor).await?;
        self.changed(workflow_run_id).await;
        Ok(response)
    }

    pub async fn continue_cursor(
        &self,
        workflow_run_id: Uuid,
        cursor: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let response =
            repository::continue_debug_cursor(self.store.as_ref(), workflow_run_id, cursor).await?;
        self.changed(workflow_run_id).await;
        Ok(response)
    }
}
