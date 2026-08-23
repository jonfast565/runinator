//! application service for importing compiled workflow packs.

use std::sync::Arc;

use runinator_blob_core::BlobStore;
use runinator_broker_core::{UiEventPublisher, emit_workflows_changed};
use runinator_models::{
    errors::SendableError,
    functions::{FunctionArtifact, FunctionVersion, NewFunctionVersion},
    pipelines::{Pipeline, PipelineBundle},
    workflows::WorkflowBundle,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, FunctionStore, NotificationStore, ScheduleStore},
};
use uuid::Uuid;

use crate::repository;

/// Applies pack-owned workflow, function, and pipeline material in the order a pack requires.
#[derive(Clone)]
pub struct PackOperations<T> {
    store: Arc<T>,
    blobs: Arc<dyn BlobStore>,
    events: UiEventPublisher,
}

impl<T> PackOperations<T> {
    pub fn new(store: Arc<T>, blobs: Arc<dyn BlobStore>, events: UiEventPublisher) -> Self {
        Self {
            store,
            blobs,
            events,
        }
    }
}

impl<T: DefinitionStore + RuntimeStore + FunctionStore + NotificationStore + ScheduleStore>
    PackOperations<T>
{
    pub async fn import_workflows(
        &self,
        bundle: WorkflowBundle,
        overwrite: bool,
    ) -> Result<WorkflowBundle, SendableError> {
        repository::import_workflow_bundle_with(self.store.as_ref(), bundle, overwrite).await
    }

    pub async fn put_function_artifact_if_absent(
        &self,
        digest: &str,
        bytes: Vec<u8>,
    ) -> Result<FunctionArtifact, SendableError> {
        repository::functions::put_artifact_if_absent(
            self.store.as_ref(),
            &self.blobs,
            digest,
            bytes,
        )
        .await
    }

    pub async fn publish_function(
        &self,
        request: &NewFunctionVersion,
    ) -> Result<FunctionVersion, SendableError> {
        repository::functions::publish_version(self.store.as_ref(), request).await
    }

    pub async fn import_pipelines(
        &self,
        bundle: &PipelineBundle,
        org_id: Option<Uuid>,
    ) -> Result<Vec<Pipeline>, SendableError> {
        repository::import_pipeline_bundle_with(self.store.as_ref(), bundle, org_id).await
    }

    pub fn workflows_changed(&self, org_id: Option<Uuid>) {
        emit_workflows_changed(&self.events, org_id);
    }
}
