//! application service for pipeline definitions, their triggers, and pipeline runs.
//!
//! HTTP adapters retain request parsing and authorization. This service owns the durable
//! operation plus the broker-backed UI invalidations and optional embedded-engine nudge that must
//! accompany it.

use std::sync::Arc;

use runinator_broker_core::{
    AppEvent, AppEventKind, Broker, EmbeddedEngineSignals, UiEventPublisher, emit,
    emit_pipeline_run, emit_workflows_changed,
};
use runinator_models::{
    errors::SendableError,
    pipelines::{Pipeline, PipelineMemberAttempt, PipelineRun, PipelineRunDetail, PipelineTrigger},
    value::Value,
    web::TaskResponse,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, ScheduleStore, WorkflowVmStore},
};
use uuid::Uuid;

use crate::repository;

/// Coordinates pipeline operations across persistence, control publication, UI publication, and
/// the optional latency hint for an embedded engine.
#[derive(Clone)]
pub struct PipelineOperations<T> {
    store: Arc<T>,
    broker: Arc<dyn Broker>,
    events: UiEventPublisher,
    signals: Option<EmbeddedEngineSignals>,
}

impl<T> PipelineOperations<T> {
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

    async fn pipeline_org(&self, pipeline_id: Uuid, fallback: Option<Uuid>) -> Option<Uuid>
    where
        T: RuntimeStore,
    {
        match repository::fetch_pipeline(self.store.as_ref(), pipeline_id).await {
            Ok(Some(pipeline)) => pipeline.org_id.or(fallback),
            _ => fallback,
        }
    }

    async fn publish_run_changed(&self, pipeline_run_id: Uuid) -> Option<Uuid>
    where
        T: RuntimeStore,
    {
        let org_id =
            repository::org_id_for_pipeline_run(self.store.as_ref(), pipeline_run_id).await;
        emit_pipeline_run(&self.events, pipeline_run_id, org_id);
        org_id
    }

    fn publish_run_activity(&self, org_id: Option<Uuid>) {
        emit(
            &self.events,
            AppEvent::new(org_id, AppEventKind::PipelineRunActivity),
        );
    }
}

impl<T: DefinitionStore + RuntimeStore + ScheduleStore + WorkflowVmStore> PipelineOperations<T> {
    pub async fn list(&self) -> Result<Vec<Pipeline>, SendableError> {
        repository::fetch_pipelines(self.store.as_ref()).await
    }

    pub async fn fetch(&self, pipeline_id: Uuid) -> Result<Option<Pipeline>, SendableError> {
        repository::fetch_pipeline(self.store.as_ref(), pipeline_id).await
    }

    pub async fn save(&self, pipeline: &Pipeline) -> Result<Pipeline, SendableError> {
        let saved = repository::upsert_pipeline(self.store.as_ref(), pipeline).await?;
        emit_workflows_changed(&self.events, saved.org_id);
        Ok(saved)
    }

    /// Updates retain the previously stored organization, preventing a caller from re-tenanting
    /// the pipeline by changing the submitted payload.
    pub async fn update(
        &self,
        pipeline_id: Uuid,
        mut pipeline: Pipeline,
    ) -> Result<Option<Pipeline>, SendableError> {
        let Some(existing) = self.fetch(pipeline_id).await? else {
            return Ok(None);
        };
        pipeline.id = Some(pipeline_id);
        pipeline.org_id = existing.org_id;
        self.save(&pipeline).await.map(Some)
    }

    pub async fn delete(
        &self,
        pipeline_id: Uuid,
        fallback_org_id: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let org_id = self.pipeline_org(pipeline_id, fallback_org_id).await;
        let response = repository::delete_pipeline(self.store.as_ref(), pipeline_id).await?;
        emit_workflows_changed(&self.events, org_id);
        Ok(response)
    }

    pub async fn list_triggers(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<PipelineTrigger>, SendableError> {
        repository::fetch_pipeline_triggers(self.store.as_ref(), pipeline_id).await
    }

    pub async fn save_trigger(
        &self,
        trigger: &PipelineTrigger,
        fallback_org_id: Option<Uuid>,
    ) -> Result<PipelineTrigger, SendableError> {
        let saved = repository::upsert_pipeline_trigger(self.store.as_ref(), trigger).await?;
        emit_workflows_changed(
            &self.events,
            self.pipeline_org(saved.pipeline_id, fallback_org_id).await,
        );
        Ok(saved)
    }

    pub async fn delete_trigger(
        &self,
        trigger_id: Uuid,
        fallback_org_id: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let org_id = match repository::fetch_pipeline_trigger(self.store.as_ref(), trigger_id).await
        {
            Ok(Some(trigger)) => {
                self.pipeline_org(trigger.pipeline_id, fallback_org_id)
                    .await
            }
            _ => fallback_org_id,
        };
        let response = repository::delete_pipeline_trigger(self.store.as_ref(), trigger_id).await?;
        emit_workflows_changed(&self.events, org_id);
        Ok(response)
    }

    pub async fn create_run(
        &self,
        pipeline_id: Uuid,
        parameters: Value,
        actor_display_name: Option<String>,
    ) -> Result<PipelineRun, SendableError> {
        let run = repository::create_manual_pipeline_run(
            self.store.as_ref(),
            pipeline_id,
            parameters,
            None,
            actor_display_name,
        )
        .await?;
        let org_id = self.publish_run_changed(run.id).await;
        self.publish_run_activity(org_id);
        self.nudge_workflow_vm();
        Ok(run)
    }

    pub async fn create_run_for_trigger(
        &self,
        trigger_id: Uuid,
        parameters: Value,
        actor_display_name: Option<String>,
    ) -> Result<PipelineRun, SendableError> {
        let run = repository::create_pipeline_run_for_trigger(
            self.store.as_ref(),
            trigger_id,
            parameters,
            None,
            actor_display_name,
        )
        .await?;
        let org_id = self.publish_run_changed(run.id).await;
        self.publish_run_activity(org_id);
        self.nudge_workflow_vm();
        Ok(run)
    }

    pub async fn list_recent_runs(&self, limit: i64) -> Result<Vec<PipelineRun>, SendableError> {
        repository::fetch_recent_pipeline_runs(self.store.as_ref(), limit).await
    }

    pub async fn fetch_run_detail(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<Option<PipelineRunDetail>, SendableError> {
        repository::fetch_pipeline_run_detail(self.store.as_ref(), pipeline_run_id).await
    }

    pub async fn delete_run(&self, pipeline_run_id: Uuid) -> Result<TaskResponse, SendableError> {
        repository::delete_pipeline_run(self.store.as_ref(), pipeline_run_id).await
    }

    pub async fn cancel_run(&self, pipeline_run_id: Uuid) -> Result<TaskResponse, SendableError> {
        let response = repository::cancel_pipeline_run(
            self.store.as_ref(),
            self.broker.as_ref(),
            pipeline_run_id,
        )
        .await?;
        let org_id = self.publish_run_changed(pipeline_run_id).await;
        self.publish_run_activity(org_id);
        Ok(response)
    }

    pub async fn pause_run(&self, pipeline_run_id: Uuid) -> Result<TaskResponse, SendableError> {
        let response = repository::pause_pipeline_run(self.store.as_ref(), pipeline_run_id).await?;
        self.publish_run_changed(pipeline_run_id).await;
        self.nudge_workflow_vm();
        Ok(response)
    }

    pub async fn resume_run(&self, pipeline_run_id: Uuid) -> Result<TaskResponse, SendableError> {
        let response =
            repository::resume_pipeline_run(self.store.as_ref(), pipeline_run_id).await?;
        self.publish_run_changed(pipeline_run_id).await;
        self.nudge_workflow_vm();
        Ok(response)
    }

    pub async fn resolve_run_inquiry(
        &self,
        pipeline_run_id: Uuid,
        continue_pipeline: bool,
        resolved_by: Option<String>,
        message: Option<String>,
    ) -> Result<PipelineRun, SendableError> {
        let run = repository::resolve_pipeline_run_inquiry(
            self.store.as_ref(),
            pipeline_run_id,
            continue_pipeline,
            resolved_by,
            message,
        )
        .await?;
        let org_id = self.publish_run_changed(pipeline_run_id).await;
        self.publish_run_activity(org_id);
        self.nudge_workflow_vm();
        Ok(run)
    }

    pub async fn retry_member(
        &self,
        pipeline_run_id: Uuid,
        member_key: String,
        parameter_override: Value,
    ) -> Result<PipelineMemberAttempt, SendableError> {
        let attempt = repository::retry_pipeline_run_member(
            self.store.as_ref(),
            pipeline_run_id,
            member_key,
            parameter_override,
        )
        .await?;
        self.publish_run_changed(pipeline_run_id).await;
        self.nudge_workflow_vm();
        Ok(attempt)
    }
}
