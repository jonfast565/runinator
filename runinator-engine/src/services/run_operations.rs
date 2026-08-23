//! application service for commands that create or change workflow runs.
//!
//! The HTTP surface supplies request parsing and authorization. This service owns the durable
//! command, its broker side effect, and the broker-backed UI notification so those three actions
//! cannot drift between transports.

use std::sync::Arc;

use runinator_broker_core::{Broker, EmbeddedEngineSignals, UiEventPublisher, emit_workflow_run};
use runinator_models::{
    errors::SendableError,
    interrupt::InterruptSource,
    replicas::WorkflowRunProvenance,
    runs::{NewRunChunk, RunChunk, RunStatus, RunSummary},
    value::Value,
    web::TaskResponse,
    workflow_state::WorkflowExecutionState,
    workflows::{WorkflowRun, WorkflowStatus},
};
use runinator_store::{
    RuntimeStore,
    roles::{RunStore, ScheduleStore, TaskRunStore, WorkflowVmStore},
};
use uuid::Uuid;

use crate::repository;

/// Coordinates a workflow-run command across persistence, control publication, UI publication,
/// and the optional latency hint for an embedded engine.
#[derive(Clone)]
pub struct RunOperations<T> {
    store: Arc<T>,
    broker: Arc<dyn Broker>,
    events: UiEventPublisher,
    signals: Option<EmbeddedEngineSignals>,
}

impl<T> RunOperations<T> {
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

    fn nudge_workflow_vm(&self) {
        if let Some(signals) = &self.signals {
            signals.nudge_workflow_vm();
        }
    }
}

impl<T: RuntimeStore + WorkflowVmStore + RunStore + ScheduleStore + TaskRunStore> RunOperations<T> {
    /// Start a run from a workflow definition and publish its invalidation after it is durable.
    pub async fn create(
        &self,
        workflow_id: Uuid,
        parameters: Value,
        debug: bool,
        name: Option<String>,
        provenance: WorkflowRunProvenance,
    ) -> Result<WorkflowRun, SendableError> {
        let run = repository::create_workflow_run(
            self.store.as_ref(),
            workflow_id,
            parameters,
            debug,
            name,
            provenance,
        )
        .await?;
        self.publish_run_changed(run.id).await;
        self.nudge_workflow_vm();
        Ok(run)
    }

    /// Start a run through a materialized trigger, retaining its trigger provenance.
    pub async fn create_for_trigger(
        &self,
        trigger_id: Uuid,
        parameters: Value,
        debug: bool,
        pipeline_run_id: Option<Uuid>,
        actor_display_name: Option<String>,
    ) -> Result<WorkflowRun, SendableError> {
        let run = repository::create_workflow_run_for_trigger(
            self.store.as_ref(),
            trigger_id,
            parameters,
            debug,
            pipeline_run_id,
            actor_display_name,
        )
        .await?;
        self.publish_run_changed(run.id).await;
        self.nudge_workflow_vm();
        Ok(run)
    }

    /// Cancel a run and send best-effort executor cancellation controls after the durable update.
    pub async fn cancel(&self, workflow_run_id: Uuid) -> Result<TaskResponse, SendableError> {
        let response = repository::cancel_workflow_run(
            self.store.as_ref(),
            self.broker.as_ref(),
            workflow_run_id,
        )
        .await?;
        self.publish_run_changed(workflow_run_id).await;
        self.nudge_workflow_vm();
        Ok(response)
    }

    pub async fn pause(&self, workflow_run_id: Uuid) -> Result<TaskResponse, SendableError> {
        let response = repository::pause_workflow_run(self.store.as_ref(), workflow_run_id).await?;
        self.publish_run_changed(workflow_run_id).await;
        Ok(response)
    }

    pub async fn resume(&self, workflow_run_id: Uuid) -> Result<TaskResponse, SendableError> {
        let response =
            repository::resume_workflow_run(self.store.as_ref(), workflow_run_id).await?;
        self.publish_run_changed(workflow_run_id).await;
        Ok(response)
    }

    /// Replays a durable snapshot and publishes the newly-created run.
    pub async fn replay(
        &self,
        workflow_run_id: Uuid,
        from_step_id: Option<String>,
    ) -> Result<WorkflowRun, SendableError> {
        let run =
            repository::replay_workflow_run(self.store.as_ref(), workflow_run_id, from_step_id)
                .await?;
        self.publish_run_changed(run.id).await;
        self.nudge_workflow_vm();
        Ok(run)
    }

    pub async fn claim_for_scheduler(
        &self,
        scheduler_id: String,
        statuses: Vec<WorkflowStatus>,
        lease_until: chrono::DateTime<chrono::Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        repository::claim_workflow_runs_for_scheduler(
            self.store.as_ref(),
            scheduler_id,
            statuses,
            lease_until,
            limit,
        )
        .await
    }

    pub async fn renew_scheduler_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
        lease_until: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        repository::renew_workflow_run_claim(
            self.store.as_ref(),
            workflow_run_id,
            scheduler_id,
            lease_until,
        )
        .await
    }

    pub async fn release_scheduler_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
    ) -> Result<(), SendableError> {
        repository::release_workflow_run_claim(self.store.as_ref(), workflow_run_id, scheduler_id)
            .await
    }

    pub async fn deliver_event(
        &self,
        workflow_run_id: Uuid,
        node_id: String,
        event: Value,
    ) -> Result<TaskResponse, SendableError> {
        let response =
            repository::deliver_run_event(self.store.as_ref(), workflow_run_id, node_id, event)
                .await?;
        self.publish_run_changed(workflow_run_id).await;
        Ok(response)
    }

    pub async fn deliver_signal(
        &self,
        workflow_run_id: Uuid,
        name: String,
        payload: Value,
    ) -> Result<TaskResponse, SendableError> {
        let response =
            repository::deliver_signal(self.store.as_ref(), workflow_run_id, name, payload).await?;
        self.publish_run_changed(workflow_run_id).await;
        Ok(response)
    }

    pub async fn request_interrupt(
        &self,
        workflow_run_id: Uuid,
        source: InterruptSource,
        payload: Value,
        continuation_id: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let response = repository::request_run_interrupt(
            self.store.as_ref(),
            workflow_run_id,
            source,
            payload,
            continuation_id,
        )
        .await?;
        self.publish_run_changed(workflow_run_id).await;
        Ok(response)
    }

    pub async fn rename(
        &self,
        workflow_run_id: Uuid,
        name: Option<String>,
    ) -> Result<TaskResponse, SendableError> {
        let response =
            repository::set_workflow_run_name(self.store.as_ref(), workflow_run_id, name).await?;
        self.publish_run_changed(workflow_run_id).await;
        Ok(response)
    }

    pub async fn update_workflow_status(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<WorkflowExecutionState>,
        message: Option<String>,
    ) -> Result<TaskResponse, SendableError> {
        let response = repository::update_workflow_run_status(
            self.store.as_ref(),
            workflow_run_id,
            status,
            active_node_id,
            state,
            message,
        )
        .await?;
        self.publish_run_changed(workflow_run_id).await;
        Ok(response)
    }

    pub async fn delete(&self, workflow_run_id: Uuid) -> Result<TaskResponse, SendableError> {
        repository::delete_workflow_run(self.store.as_ref(), workflow_run_id).await
    }

    pub async fn fetch_workflow(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<WorkflowRun>, SendableError> {
        repository::fetch_workflow_run(self.store.as_ref(), workflow_run_id).await
    }

    pub async fn list_workflow_by_name(
        &self,
        name: String,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        repository::fetch_workflow_runs_by_name(self.store.as_ref(), name, open_only).await
    }

    pub async fn list_workflow_for_definition(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        repository::fetch_workflow_runs_for_workflow(self.store.as_ref(), workflow_id).await
    }

    pub async fn list_workflow_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        repository::fetch_workflow_runs_by_status(self.store.as_ref(), status).await
    }

    pub async fn list_recent_workflow(
        &self,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        repository::fetch_recent_workflow_runs(self.store.as_ref(), limit).await
    }

    pub async fn list_task_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<RunSummary>, SendableError> {
        repository::fetch_runs_by_status(self.store.as_ref(), status).await
    }

    pub async fn task_chunks(
        &self,
        run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<RunChunk>, SendableError> {
        repository::fetch_run_chunks(self.store.as_ref(), run_id, cursor, limit).await
    }

    pub async fn update_task_status(
        &self,
        run_id: Uuid,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
        org_id: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let response = repository::update_run_status(
            self.store.as_ref(),
            run_id,
            status,
            output_json,
            message,
        )
        .await?;
        runinator_broker_core::emit_task_run(&self.events, run_id, status, org_id);
        Ok(response)
    }

    pub async fn append_task_chunk(
        &self,
        run_id: Uuid,
        chunk: &NewRunChunk,
        org_id: Option<Uuid>,
    ) -> Result<RunChunk, SendableError> {
        let chunk = repository::append_run_chunk(self.store.as_ref(), run_id, chunk).await?;
        runinator_broker_core::emit(
            &self.events,
            runinator_broker_core::AppEvent::new(
                org_id,
                runinator_broker_core::AppEventKind::RunChunkAdded { run_id },
            ),
        );
        Ok(chunk)
    }

    async fn publish_run_changed(&self, workflow_run_id: Uuid) {
        let org_id =
            repository::org_id_for_workflow_run(self.store.as_ref(), workflow_run_id).await;
        emit_workflow_run(&self.events, workflow_run_id, org_id);
    }
}
