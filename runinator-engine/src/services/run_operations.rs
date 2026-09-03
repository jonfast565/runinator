//! application service for commands that create or change workflow runs.
//!
//! The HTTP surface supplies request parsing and authorization. This service owns the durable
//! command, its broker side effect, and the broker-backed UI notification so those three actions
//! cannot drift between transports.

use std::collections::BTreeSet;
use std::sync::Arc;

use runinator_broker_core::{Broker, EmbeddedEngineSignals, UiEventPublisher, emit_workflow_run};
use runinator_models::{
    auth::ResourceType,
    errors::SendableError,
    files::{FileScope, referenced_file_ids},
    interrupt::InterruptSource,
    replicas::WorkflowRunProvenance,
    value::Value,
    web::TaskResponse,
    workflow_state::WorkflowExecutionState,
    workflows::{WorkflowRun, WorkflowStatus},
};
use runinator_store::{
    RuntimeStore,
    roles::{FileStore, OrchestrationStore, RunStore, ScheduleStore, WorkflowVmStore},
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

impl<T: RuntimeStore + OrchestrationStore> RunOperations<T> {
    /// Resolve whether a workflow run is owned by a correlated pipeline execution. Keeping this
    /// traversal in the run service makes every transport apply the same managed-run guard.
    pub async fn managed_orchestration_binding(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<runinator_models::orchestration::OrchestrationBinding>, SendableError> {
        let Some(pipeline_run_id) = self
            .store
            .fetch_workflow_run(workflow_run_id)
            .await?
            .and_then(|run| run.pipeline_run_id)
        else {
            return Ok(None);
        };
        let Some(binding_id) = self
            .store
            .fetch_pipeline_run(pipeline_run_id)
            .await?
            .and_then(|run| run.orchestration_binding_id)
        else {
            return Ok(None);
        };
        self.store.fetch_orchestration_binding(binding_id).await
    }
}

impl<
    T: RuntimeStore
        + WorkflowVmStore
        + RunStore
        + ScheduleStore
        + FileStore
        + runinator_store::roles::AuthStore
        + runinator_store::roles::RbacStore,
> RunOperations<T>
{
    pub async fn fetch_workflow_definition(
        &self,
        workflow_id: Uuid,
    ) -> Result<Option<runinator_models::workflows::WorkflowDefinition>, SendableError> {
        self.store.fetch_workflow(workflow_id).await
    }

    /// Start a run from a workflow definition and publish its invalidation after it is durable.
    #[allow(
        clippy::too_many_arguments,
        reason = "run creation preserves independent provenance, ownership, file, and debug inputs at the service boundary"
    )]
    pub async fn create(
        &self,
        workflow_id: Uuid,
        parameters: Value,
        debug: bool,
        name: Option<String>,
        provenance: WorkflowRunProvenance,
        file_ids: Vec<Uuid>,
        org_id: Option<Uuid>,
        principal_id: Option<Uuid>,
    ) -> Result<WorkflowRun, SendableError> {
        let workflow = self
            .store
            .fetch_workflow(workflow_id)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "workflow not found",
                )) as SendableError
            })?;
        repository::validate_workflow_dependency_access(self.store.as_ref(), &workflow).await?;
        let supplied_file_ids = file_ids.into_iter().collect::<BTreeSet<_>>();
        let referenced_ids = referenced_file_ids(&parameters)
            .into_iter()
            .collect::<BTreeSet<_>>();
        if supplied_file_ids != referenced_ids {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "file_ids must exactly match file descriptors in workflow parameters",
            )));
        }
        for file_id in &supplied_file_ids {
            let Some(file) = self.store.fetch_file(*file_id).await? else {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "selected workflow file no longer exists",
                )));
            };
            if file.scope == FileScope::Library
                && !runinator_store::resource_access::resource_can_consume(
                    self.store.as_ref(),
                    ResourceType::Workflow,
                    workflow_id,
                    ResourceType::LibraryFile,
                    *file_id,
                )
                .await?
            {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!(
                        "workflow {workflow_id} is not permitted to use library file {file_id}"
                    ),
                )));
            }
        }
        let run = repository::create_workflow_run(
            self.store.as_ref(),
            workflow_id,
            parameters,
            debug,
            name,
            provenance,
        )
        .await?;
        if !supplied_file_ids.is_empty() {
            self.store
                .claim_staged_files(
                    &supplied_file_ids.into_iter().collect::<Vec<_>>(),
                    org_id,
                    principal_id,
                    run.id,
                )
                .await?;
        }
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
        let trigger = self
            .store
            .fetch_workflow_trigger(trigger_id)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "workflow trigger not found",
                )) as SendableError
            })?;
        let workflow = self
            .store
            .fetch_workflow(trigger.workflow_id)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "workflow not found",
                )) as SendableError
            })?;
        repository::validate_workflow_dependency_access(self.store.as_ref(), &workflow).await?;
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

    async fn publish_run_changed(&self, workflow_run_id: Uuid) {
        let org_id =
            repository::org_id_for_workflow_run(self.store.as_ref(), workflow_run_id).await;
        emit_workflow_run(&self.events, workflow_run_id, org_id);
    }
}
