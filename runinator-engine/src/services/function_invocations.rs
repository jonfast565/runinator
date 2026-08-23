//! application service for invoking packaged-function adapter workflows.

use std::sync::Arc;

use runinator_broker_core::{Broker, EmbeddedEngineSignals, UiEventPublisher, emit_workflow_run};
use runinator_models::{
    errors::SendableError,
    functions::{FunctionExport, FunctionPackage, FunctionVersion, FunctionVersionRef},
    replicas::WorkflowRunProvenance,
    value::Value,
    workflows::WorkflowRun,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, DeliveryStore, FunctionStore, WorkflowVmStore},
};
use uuid::Uuid;

use crate::repository;

/// A resolved packaged-function export and the immutable adapter workflow that executes it.
pub struct ResolvedFunctionInvocation {
    pub version: FunctionVersion,
    pub export: FunctionExport,
    pub workflow_id: Uuid,
}

/// Coordinates function invocation persistence, control publication, and UI invalidation.
#[derive(Clone)]
pub struct FunctionInvocations<T> {
    store: Arc<T>,
    broker: Arc<dyn Broker>,
    events: UiEventPublisher,
    signals: Option<EmbeddedEngineSignals>,
}

impl<T> FunctionInvocations<T> {
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

    async fn publish_run_changed(&self, run_id: Uuid)
    where
        T: RuntimeStore,
    {
        let org_id = repository::org_id_for_workflow_run(self.store.as_ref(), run_id).await;
        emit_workflow_run(&self.events, run_id, org_id);
    }
}

impl<T: DefinitionStore + DeliveryStore + FunctionStore + RuntimeStore + WorkflowVmStore>
    FunctionInvocations<T>
{
    pub async fn fetch_package(
        &self,
        org_id: Option<Uuid>,
        namespace: Option<&str>,
        name: &str,
    ) -> Result<Option<FunctionPackage>, SendableError> {
        Ok(repository::functions::fetch_package_detail(
            self.store.as_ref(),
            org_id,
            namespace,
            name,
        )
        .await?
        .map(|detail| detail.package))
    }

    pub async fn resolve_export(
        &self,
        package: FunctionPackage,
        reference: &FunctionVersionRef,
        export_name: &str,
    ) -> Result<Option<ResolvedFunctionInvocation>, SendableError> {
        let (version, export) = repository::functions::resolve_export(
            self.store.as_ref(),
            &package,
            reference,
            export_name,
        )
        .await?;
        let adapter =
            repository::function_adapters::fetch_adapter_workflow(self.store.as_ref(), export.id)
                .await?;
        let Some(adapter) = adapter else {
            return Ok(None);
        };
        Ok(Some(ResolvedFunctionInvocation {
            version,
            export,
            workflow_id: adapter.workflow_id,
        }))
    }

    pub async fn fetch_idempotency(
        &self,
        scope: String,
        key: String,
    ) -> Result<Option<Value>, SendableError> {
        repository::fetch_idempotency_key(self.store.as_ref(), scope, key).await
    }

    pub async fn put_idempotency(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> Result<Value, SendableError> {
        repository::put_idempotency_key(self.store.as_ref(), scope, key, result).await
    }

    pub async fn start(
        &self,
        workflow_id: Uuid,
        input: Value,
        name: Option<String>,
        provenance: WorkflowRunProvenance,
    ) -> Result<WorkflowRun, SendableError> {
        let run = repository::create_workflow_run(
            self.store.as_ref(),
            workflow_id,
            input,
            false,
            name,
            provenance,
        )
        .await?;
        self.publish_run_changed(run.id).await;
        self.nudge_workflow_vm();
        Ok(run)
    }

    pub async fn fetch_run(&self, run_id: Uuid) -> Result<Option<WorkflowRun>, SendableError> {
        repository::fetch_workflow_run(self.store.as_ref(), run_id).await
    }

    pub async fn cancel(&self, run_id: Uuid) -> Result<(), SendableError> {
        repository::cancel_workflow_run(self.store.as_ref(), self.broker.as_ref(), run_id).await?;
        self.publish_run_changed(run_id).await;
        self.nudge_workflow_vm();
        Ok(())
    }
}
